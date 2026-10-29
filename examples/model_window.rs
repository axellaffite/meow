//! Shows a 3D creature in a transparent window over the desktop, as the cat
//! is shown: it idles, nods when clicked, and lets clicks on its empty
//! pixels through to the apps below. Ctrl-C quits.
//! `cargo run --example model_window -- [creature.glb]`
//! (the test fixture by default).

// The app's UI module, borrowed whole: most of it is unused here.
#![allow(dead_code, unused_imports)]

#[path = "../src/ui/mod.rs"]
mod ui;

use std::cell::RefCell;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Instant;

use block2::RcBlock;
use glam::Vec2;
use meow::creature::CreatureAsset;
use meow::creature::frame::Frame;
use meow::motion::{Animator, Placement, Play};
use meow::render::{HitTest, Pending, Quality, Share};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEvent, NSPanel,
    NSStatusWindowLevel, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSTimer};

use ui::metal_view::{LiveView, current_quality, follow_pointer};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mini_creature.glb"
);
/// The window, points.
const SIZE: NSSize = NSSize::new(180.0, 220.0);
const TICK_SECONDS: f64 = 1.0 / 30.0;

/// What the tick works on.
struct Stage {
    asset: CreatureAsset,
    pending: Pending,
    live: Option<LiveView>,
    animator: Animator,
    placement: Placement,
    frame: Frame,
    hit: HitTest,
    started: Instant,
    last: Instant,
    /// The mouse was down last tick.
    pressed: bool,
}

fn main() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("model_window must start on the main thread");
        std::process::exit(1);
    };
    let path = std::env::args().nth(1).unwrap_or_else(|| FIXTURE.into());
    let asset = match CreatureAsset::load(Path::new(&path)) {
        Ok(asset) => asset,
        Err(error) => {
            eprintln!("model_window: {error}");
            std::process::exit(1);
        }
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let panel = panel(mtm);
    let content = panel.contentView().expect("a panel has a content view");
    let mut animator = Animator::new(&asset);
    animator.play("idle", Play::Loop, 1.0, 0.0);
    let placement = Placement::new(Vec2::new(SIZE.width as f32 / 2.0, 16.0), 180.0);
    let frame = animator.update(0.0, &placement);
    let stage = Rc::new(RefCell::new(Stage {
        pending: Pending::spawn(asset.clone()),
        asset,
        live: None,
        animator,
        placement,
        frame,
        hit: HitTest::default(),
        started: Instant::now(),
        last: Instant::now(),
        pressed: false,
    }));

    let ticked = Rc::clone(&stage);
    let window = panel.clone();
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
        if let Ok(mut stage) = ticked.try_borrow_mut() {
            tick(mtm, &mut stage, &window, &content);
        }
    });
    // SAFETY: the block only touches main-thread state and the timer fires
    // on the main run loop.
    unsafe {
        let timer = NSTimer::timerWithTimeInterval_repeats_block(TICK_SECONDS, true, &block);
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
    }
    panel.orderFrontRegardless();
    println!("{path}: click the creature to make it nod; Ctrl-C quits");
    app.run();
}

/// A borderless transparent panel over every window, as the cat's.
fn panel(mtm: MainThreadMarker) -> Retained<NSPanel> {
    let rect = NSRect::new(NSPoint::new(200.0, 200.0), SIZE);
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        rect,
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: the panel lives until the app quits.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    panel.setHidesOnDeactivate(false);
    panel.setLevel(NSStatusWindowLevel);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setContentView(Some(&NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), SIZE),
    )));
    panel
}

fn tick(mtm: MainThreadMarker, stage: &mut Stage, window: &NSPanel, content: &NSView) {
    if stage.live.is_none() {
        match stage.pending.poll() {
            Share::Compiling => return,
            Share::Ready(renderer) => stage.live = LiveView::new(mtm, content, *renderer),
            Share::Unavailable => {}
        }
        if stage.live.is_none() {
            eprintln!("model_window: no Metal renderer");
            std::process::exit(1);
        }
    }
    let now = Instant::now();
    let dt = now.duration_since(stage.last).as_secs_f32();
    stage.last = now;

    // A click on the creature (the window only gets the mouse there).
    let pressed = NSEvent::pressedMouseButtons() != 0;
    if pressed && !stage.pressed && !window.ignoresMouseEvents() {
        stage.animator.play("nod", Play::Once, 1.0, 0.15);
        stage.animator.play("idle", Play::Loop, 1.0, 0.25);
    }
    stage.pressed = pressed;

    let Stage {
        asset,
        live: Some(live),
        animator,
        placement,
        frame,
        hit,
        started,
        ..
    } = stage
    else {
        return;
    };
    animator.update_into(dt, placement, frame);
    let time = started.elapsed().as_secs_f32();
    if let Err(error) = live.present(
        frame,
        current_quality(Quality::Balanced),
        Vec2::ZERO,
        time,
        false,
    ) {
        eprintln!("model_window: {error}");
        std::process::exit(1);
    }
    let to_window = live.to_window(frame);
    follow_pointer(window, |p| hit.hits(asset, frame, to_window, p));
}
