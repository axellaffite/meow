//! Transparent, click-through-free panel that shows the cat sprite: pixel
//! art, or in the Classic style the flat vector cat.

use std::cell::{Cell, RefCell};

use meow::sprites::{
    self, CANVAS_H, CANVAS_W, Coat, Facing, Frame, Look, PROP_H, PROP_W, Prop, SCRUFF,
};
use meow::vector_cat;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSCursor, NSEvent, NSGraphicsContext, NSMenu,
    NSPanel, NSStatusWindowLevel, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::events::{UiEvent, push};
use super::screens;

pub const SCALE: f64 = meow::sprites::SCALE;
pub const WIDTH: f64 = CANVAS_W as f64 * SCALE;
pub const HEIGHT: f64 = CANVAS_H as f64 * SCALE;
/// Mouse travel (in points) that turns a click into a drag.
const DRAG_THRESHOLD: f64 = 3.0;

/// A mouse press on the cat, in screen coordinates.
#[derive(Clone, Copy)]
struct Press {
    mouse: NSPoint,
    window_origin: NSPoint,
    dragging: bool,
}

/// What a Classic style view draws as vectors, in place of a pixel frame.
#[derive(Clone, Copy)]
enum Vector {
    Cat(Look, Coat),
    Prop(Prop, Facing),
}

#[derive(Default)]
pub struct CatViewIvars {
    frame: RefCell<Option<Frame>>,
    vector: Cell<Option<Vector>>,
    press: Cell<Option<Press>>,
    /// Last mouse position over the cat, to measure strokes.
    hover: Cell<Option<NSPoint>>,
    /// How far (points, screen directions) the sprite is drawn from the
    /// window's corner: off it when only part of the cat is on this screen.
    shift: Cell<(f64, f64)>,
    /// Shows a hairball: dragging over it scrubs it, clicking dabs at it.
    wipes: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowCatView"]
    #[ivars = CatViewIvars]
    pub struct CatView;

    impl CatView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            let Some(window) = self.window() else { return };
            self.ivars().press.set(Some(Press {
                mouse: NSEvent::mouseLocation(),
                window_origin: window.frame().origin,
                dragging: false,
            }));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, _event: &NSEvent) {
            let Some(mut press) = self.ivars().press.get() else { return };
            let mouse = NSEvent::mouseLocation();
            let (dx, dy) = (mouse.x - press.mouse.x, mouse.y - press.mouse.y);
            if self.ivars().wipes.get() {
                push(UiEvent::WipeHairball(dx.hypot(dy)));
                press.mouse = mouse;
                press.dragging = true;
                self.ivars().press.set(Some(press));
                return;
            }
            if !press.dragging && dx.hypot(dy) >= DRAG_THRESHOLD {
                press.dragging = true;
                // Hang the cat by its scruff right under the cursor.
                press.window_origin = NSPoint::new(
                    press.mouse.x - SCRUFF.0 as f64 * SCALE,
                    press.mouse.y - (HEIGHT - (SCRUFF.1 as f64 + 0.5) * SCALE),
                );
                NSCursor::closedHandCursor().set();
                push(UiEvent::Grab);
            }
            if press.dragging {
                push(UiEvent::DragTo {
                    x: press.window_origin.x + dx,
                    y: press.window_origin.y + dy,
                });
            }
            self.ivars().press.set(Some(press));
        }

        /// Moving the mouse over the cat, without a button, strokes it.
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, _event: &NSEvent) {
            if self.ivars().wipes.get() {
                return;
            }
            let mouse = NSEvent::mouseLocation();
            if let Some(last) = self.ivars().hover.replace(Some(mouse)) {
                let distance = (mouse.x - last.x).hypot(mouse.y - last.y);
                push(UiEvent::Stroke(distance));
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hover.set(None);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let press = self.ivars().press.take();
            if self.ivars().wipes.get() {
                if press.is_some_and(|press| !press.dragging) {
                    push(UiEvent::TapHairball);
                }
                return;
            }
            match press {
                Some(Press { dragging: true, .. }) => {
                    NSCursor::arrowCursor().set();
                    push(UiEvent::Drop);
                }
                Some(_) => push(UiEvent::CatClicked),
                None => {}
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            match self.ivars().vector.get() {
                Some(vector) => self.draw_vector(vector),
                None => self.draw_pixels(),
            }
        }
    }
);

impl CatView {
    fn new(mtm: MainThreadMarker, rect: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CatViewIvars::default());
        // SAFETY: NSView's designated initializer, called once on a fresh allocation.
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: rect] };
        // Layer backing gives a cleared buffer on every redraw of the transparent window.
        view.setWantsLayer(true);
        let options = NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect;
        let owner: &AnyObject = &view;
        // SAFETY: the view owns the area and outlives it; no user info is attached.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                rect,
                options,
                Some(owner),
                None,
            )
        };
        view.addTrackingArea(&area);
        view
    }

    pub fn show(&self, frame: Frame) {
        self.ivars().vector.set(None);
        self.ivars().frame.replace(Some(frame));
        self.setNeedsDisplay(true);
    }

    /// Shows the cat in `look` and `coat`: the pixel sprite, or the flat
    /// vector cat in the Classic style.
    pub fn show_look(&self, look: Look, coat: Coat) {
        if meow::style::is_classic() {
            self.show_vector(Vector::Cat(look, coat));
        } else {
            self.show(sprites::render(&look, coat));
        }
    }

    /// Shows `prop` as `show_look` shows the cat.
    pub fn show_prop(&self, prop: Prop, facing: Facing) {
        if meow::style::is_classic() {
            self.show_vector(Vector::Prop(prop, facing));
        } else {
            self.show(sprites::render_prop(prop, facing));
        }
    }

    fn show_vector(&self, vector: Vector) {
        self.ivars().vector.set(Some(vector));
        self.ivars().frame.replace(None);
        self.setNeedsDisplay(true);
    }

    fn set_shift(&self, shift: (f64, f64)) {
        if self.ivars().shift.replace(shift) != shift {
            self.setNeedsDisplay(true);
        }
    }

    fn draw_vector(&self, vector: Vector) {
        let Some(context) = NSGraphicsContext::currentContext() else {
            return;
        };
        let ctx = context.CGContext();
        let c = Some(&*ctx);
        // The view is flipped, so the context is already y down, as the
        // drawing expects; it only needs moving by the shift and scaling.
        let (dx, dy) = self.ivars().shift.get();
        CGContext::save_g_state(c);
        CGContext::translate_ctm(c, dx, -dy);
        CGContext::scale_ctm(c, SCALE, SCALE);
        match vector {
            Vector::Cat(look, coat) => vector_cat::draw_look(&ctx, &look, coat),
            Vector::Prop(prop, facing) => vector_cat::draw_prop(&ctx, prop, facing),
        }
        CGContext::restore_g_state(c);
    }

    fn draw_pixels(&self) {
        let frame = self.ivars().frame.borrow();
        let Some(frame) = frame.as_ref() else { return };
        // The view is flipped: up on screen is down here.
        let (dx, dy) = self.ivars().shift.get();
        for y in 0..frame.height {
            for x in 0..frame.width {
                let Some([r, g, b]) = frame.get(x, y) else {
                    continue;
                };
                let channel = |v: u8| f64::from(v) / 255.0;
                NSColor::colorWithSRGBRed_green_blue_alpha(channel(r), channel(g), channel(b), 1.0)
                    .setFill();
                let origin = NSPoint::new(x as f64 * SCALE + dx, y as f64 * SCALE - dy);
                NSBezierPath::fillRect(NSRect::new(origin, NSSize::new(SCALE, SCALE)));
            }
        }
    }
}

pub struct CatWindow {
    pub panel: Retained<NSPanel>,
    pub view: Retained<CatView>,
}

impl CatWindow {
    pub fn new(mtm: MainThreadMarker, menu: &NSMenu) -> Self {
        let window = Self::panel(mtm, NSSize::new(WIDTH, HEIGHT));
        // SAFETY: the menu is an app-lifetime object retained by the view.
        unsafe { window.view.setMenu(Some(menu)) };
        window.panel.orderFrontRegardless();
        window
    }

    /// A window for a prop next to the cat (its bowl, a mouse): it lets
    /// clicks through and starts hidden.
    pub fn prop(mtm: MainThreadMarker) -> Self {
        let size = NSSize::new(PROP_W as f64 * SCALE, PROP_H as f64 * SCALE);
        let window = Self::panel(mtm, size);
        window.panel.setIgnoresMouseEvents(true);
        window
    }

    /// A window for the hairball on the floor: it takes clicks and drags, to
    /// wipe it away, and starts hidden.
    pub fn hairball(mtm: MainThreadMarker) -> Self {
        let size = NSSize::new(PROP_W as f64 * SCALE, PROP_H as f64 * SCALE);
        let window = Self::panel(mtm, size);
        window.view.ivars().wipes.set(true);
        window
    }

    /// A window for the part of the cat over the next screen while it crosses
    /// over: it lets clicks through and starts hidden.
    pub fn overflow(mtm: MainThreadMarker) -> Self {
        let window = Self::panel(mtm, NSSize::new(WIDTH, HEIGHT));
        window.panel.setIgnoresMouseEvents(true);
        window
    }

    fn panel(mtm: MainThreadMarker, size: NSSize) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), size);
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: the panel is owned by `CatWindow` for the app's lifetime.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        // Mouse moves over the cat are how it gets petted.
        panel.setAcceptsMouseMovedEvents(true);
        panel.setHidesOnDeactivate(false);
        panel.setLevel(NSStatusWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );

        let view = CatView::new(mtm, rect);
        panel.setContentView(Some(&view));
        Self { panel, view }
    }

    pub fn move_to(&self, x: f64, y: f64) {
        self.panel
            .setFrameOrigin(NSPoint::new(x.round(), y.round()));
    }

    /// Shows the sprite at `origin` (screen coordinates), or the part of it on
    /// the screen with `frame`: the window stays whole on that screen, with the
    /// sprite drawn off its corner. A window across two displays shows on only
    /// one of them, and flickers as it changes over.
    pub fn show_on(&self, frame: NSRect, origin: NSPoint) {
        let origin = NSPoint::new(origin.x.round(), origin.y.round());
        let at = screens::keep_whole_on(frame, origin, self.panel.frame().size);
        let at = NSPoint::new(at.x.round(), at.y.round());
        self.panel.setFrameOrigin(at);
        self.view.set_shift((origin.x - at.x, origin.y - at.y));
    }

    pub fn frame(&self) -> NSRect {
        self.panel.frame()
    }
}
