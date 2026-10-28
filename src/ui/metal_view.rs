//! A 3D creature drawn by Metal in a transparent window: a `CAMetalLayer`
//! filling the window's view ([`LiveView`]), drawn on the app's tick and
//! only when something changed (`render::Pacer`), and a window that takes
//! the mouse only where the creature is ([`follow_pointer`]), so clicks on
//! its empty pixels reach the apps below.
//!
//! Nothing here waits on the main thread: the renderer comes from
//! `render::Pending`, compiled on a worker thread, and a frame the GPU is
//! too busy for is skipped, not waited for.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use block2::RcBlock;
use glam::{Mat4, Vec2};
use meow::creature::frame::Frame;
use meow::render::{Canvas, Pacer, Quality, RenderSettings, Renderer, Theme};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSAutoresizingMaskOptions, NSEvent, NSView, NSWindow, NSWindowOcclusionState,
};
use objc2_core_foundation::CGSize;
use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};
use objc2_foundation::{NSArray, NSPoint, NSProcessInfo};
use objc2_metal::{MTLCommandBuffer, MTLCommandQueue, MTLDevice};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

/// Frames sent to the GPU and not finished yet, at most: past it a tick
/// skips its frame rather than wait for a drawable on the main thread.
const MAX_IN_FLIGHT: usize = 2;

/// The view a [`LiveView`] draws in: it hosts the `CAMetalLayer` and lets
/// every mouse event through to the view under it. No ivars: a class
/// holding `glam` values must box them (see `class_tests.rs`).
#[derive(Default)]
pub struct MetalHostIvars {}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowMetalHostView"]
    #[ivars = MetalHostIvars]
    pub struct MetalHostView;

    impl MetalHostView {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            false
        }
    }
);

/// A creature drawn by Metal in one window.
pub struct LiveView {
    host: Retained<MetalHostView>,
    layer: Retained<CAMetalLayer>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    renderer: Renderer,
    pacer: Pacer,
    /// How far the camera looks down, degrees.
    pub pitch: f32,
    /// Frames committed and not yet done by the GPU (see [`MAX_IN_FLIGHT`]).
    in_flight: Arc<AtomicUsize>,
}

impl LiveView {
    /// Fills `parent` with a Metal layer drawn by `renderer`.
    pub fn new(mtm: MainThreadMarker, parent: &NSView, renderer: Renderer) -> Option<Self> {
        let queue = renderer.device().newCommandQueue()?;
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(renderer.device()));
        layer.setPixelFormat(Renderer::PIXEL_FORMAT);
        layer.setFramebufferOnly(true);
        layer.setOpaque(false);
        // The renderer writes sRGB values: say so, so they are not
        // colour-matched as the display's own.
        // SAFETY: a system colour space name constant.
        if let Some(srgb) = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })) {
            layer.setColorspace(Some(&srgb));
        }
        // One on screen and at most MAX_IN_FLIGHT being drawn: with three,
        // one is always free when `present` asks, so `nextDrawable` does not
        // block the tick (it waits up to a second when none is).
        layer.setMaximumDrawableCount(3);
        layer.setAllowsNextDrawableTimeout(true);
        let bounds = parent.bounds();
        let this = MetalHostView::alloc(mtm).set_ivars(MetalHostIvars::default());
        // SAFETY: NSView's designated initializer, called once on a fresh allocation.
        let host: Retained<MetalHostView> =
            unsafe { msg_send![super(this), initWithFrame: bounds] };
        // Layer hosting: the layer is set before asking for one.
        host.setLayer(Some(&layer));
        host.setWantsLayer(true);
        host.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        parent.addSubview(&host);
        Some(Self {
            host,
            layer,
            queue,
            renderer,
            pacer: Pacer::default(),
            pitch: RenderSettings::default().pitch,
            in_flight: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Draws `frame` if anything changed since the last frame drawn;
    /// `Ok(true)` when it drew. `shift` moves the drawing off the window's
    /// corner (points), `time` is the caller's clock, `calm` says the
    /// creature only breathes (it is then drawn at the quality's idle rate).
    /// A window hidden or covered draws nothing, and draws at once when
    /// seen again.
    pub fn present(
        &mut self,
        frame: &Frame,
        quality: Quality,
        shift: Vec2,
        time: f32,
        calm: bool,
    ) -> Result<bool, String> {
        let Some(window) = self.host.window() else {
            return Ok(false);
        };
        // Covered, or its display asleep or locked: `nextDrawable` would
        // wait on the main thread for a drawable the compositor does not
        // hand back.
        if !window.isVisible()
            || !window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
        {
            self.pacer.reset();
            return Ok(false);
        }
        let scale = window.backingScaleFactor().max(1.0);
        let bounds = self.host.bounds().size;
        let pixels = (
            (bounds.width * scale).round().max(1.0) as usize,
            (bounds.height * scale).round().max(1.0) as usize,
        );
        let look = Canvas {
            theme: theme_of(&self.host),
            quality,
            shift,
            pixels,
        };
        if !self.pacer.should_draw(frame, &look, time, calm) {
            return Ok(false);
        }
        if self.in_flight.load(Ordering::Acquire) >= MAX_IN_FLIGHT {
            // The GPU is behind: skip this tick; the pacer still holds the
            // last frame drawn, so the next tick draws.
            return Ok(false);
        }
        if (self.layer.contentsScale() - scale).abs() > 1e-6 {
            self.layer.setContentsScale(scale);
        }
        let size = CGSize::new(pixels.0 as f64, pixels.1 as f64);
        let current = self.layer.drawableSize();
        if current.width != size.width || current.height != size.height {
            self.layer.setDrawableSize(size);
        }
        let Some(drawable) = self.layer.nextDrawable() else {
            // None free in time: try again on the next tick.
            self.pacer.reset();
            return Ok(false);
        };
        let command_buffer = self
            .queue
            .commandBuffer()
            .ok_or("no Metal command buffer")?;
        self.renderer.set_offset(shift);
        let settings = RenderSettings {
            theme: look.theme,
            quality,
            time,
            scale: scale as f32,
            pitch: self.pitch,
        };
        self.renderer
            .draw(&command_buffer, &drawable.texture(), frame, &settings)?;
        command_buffer.presentDrawable(ProtocolObject::from_ref(&*drawable));
        let in_flight = Arc::clone(&self.in_flight);
        // Called on a Metal thread when the GPU is done with the frame.
        let done = RcBlock::new(
            move |_: std::ptr::NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                in_flight.fetch_sub(1, Ordering::AcqRel);
            },
        );
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        // SAFETY: the block lives as long as Metal holds it (it copies it).
        unsafe { command_buffer.addCompletedHandler(RcBlock::as_ptr(&done)) };
        command_buffer.commit();
        self.pacer.drew(frame, &look, time);
        Ok(true)
    }

    /// Creature space to window points as drawn: what `render::HitTest`
    /// takes to find the pointer on `frame`.
    pub fn to_window(&self, frame: &Frame) -> Mat4 {
        let settings = RenderSettings {
            pitch: self.pitch,
            ..RenderSettings::default()
        };
        self.renderer.to_window(frame, &settings)
    }

    /// Forgets what it drew: the next call draws, whatever changed.
    pub fn forget(&mut self) {
        self.pacer.reset();
    }

    /// Takes the layer out of its window.
    pub fn remove(&self) {
        self.host.removeFromSuperview();
    }
}

/// The macOS appearance `view` is drawn in, as the renderer's theme.
pub fn theme_of(view: &NSView) -> Theme {
    // SAFETY: AppKit's appearance-name constants are immutable statics.
    let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let names = NSArray::from_slice(&[aqua, dark]);
    match view
        .effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names)
    {
        Some(name) if &*name == dark => Theme::Dark,
        _ => Theme::Light,
    }
}

/// How much the renderer may spend now: Battery in Low Power Mode, else
/// the quality `chosen`.
pub fn current_quality(chosen: Quality) -> Quality {
    if NSProcessInfo::processInfo().isLowPowerModeEnabled() {
        Quality::Battery
    } else {
        chosen
    }
}

/// Lets `window` take the mouse only where `hits` says the pointer (window
/// points, y up) is on the creature, so clicks on its empty pixels reach the
/// apps below: a Metal layer does not give that for free. A press under way
/// keeps the mouse, so a drag that slips off the creature goes on. Call it
/// on every tick.
pub fn follow_pointer(window: &NSWindow, mut hits: impl FnMut(Vec2) -> bool) {
    let taking = !window.ignoresMouseEvents();
    let on = (taking && NSEvent::pressedMouseButtons() != 0) || {
        let mouse = NSEvent::mouseLocation();
        let origin = window.frame().origin;
        hits(Vec2::new(
            (mouse.x - origin.x) as f32,
            (mouse.y - origin.y) as f32,
        ))
    };
    if taking != on {
        window.setIgnoresMouseEvents(!on);
    }
}
