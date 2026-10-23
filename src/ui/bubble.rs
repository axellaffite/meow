//! The main window drawn as the cat's speech bubble: a pixel-art box with
//! stepped corners, a drop shadow, and a tail pointing down at the cat; in
//! the classic style, a rounded box with a soft shadow.

use std::cell::Cell;

use meow::style;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSGraphicsContext, NSPanel, NSShadow, NSView,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::theme;

/// One "pixel" of the bubble, matching the cat sprite's scale.
pub const PIXEL: f64 = 3.0;
/// Corner radius of the classic style's bubble.
const CLASSIC_RADIUS: f64 = 14.0;
const SHADOW: f64 = 2.0 * PIXEL;
const TAIL_ROWS: usize = 4;
const TAIL_HEIGHT: f64 = TAIL_ROWS as f64 * PIXEL;
/// Gap between the outline and the content.
pub const PADDING: f64 = 2.0 * PIXEL;

define_class!(
    /// Borderless panel that can still take keyboard focus for the inputs.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowBubblePanel"]
    pub struct BubblePanel;

    impl BubblePanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }
    }
);

impl BubblePanel {
    pub fn new(mtm: MainThreadMarker, size: NSSize) -> Retained<Self> {
        let style = NSWindowStyleMask::Borderless
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::NonactivatingPanel;
        let this = Self::alloc(mtm).set_ivars(());
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
        // SAFETY: NSWindow's designated initializer with a valid rect, style and backing.
        unsafe {
            msg_send![super(this), initWithContentRect: frame, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
        }
    }
}

pub struct BubbleIvars {
    /// Tail position from the bubble's left edge.
    tail_x: Cell<f64>,
    /// Right edge of the sidebar column, from the bubble's left edge.
    sidebar_right: Cell<f64>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowBubbleView"]
    #[ivars = BubbleIvars]
    pub struct BubbleView;

    impl BubbleView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

impl BubbleView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BubbleIvars {
            tail_x: Cell::new(frame.size.width / 2.0),
            sidebar_right: Cell::new(0.0),
        });
        // SAFETY: NSView's designated initializer, called once on a fresh allocation.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Where content goes, inside the outline and above the tail.
    pub fn content_rect(&self) -> NSRect {
        let body = body_rect(self.bounds());
        inset(body, PIXEL + PADDING)
    }

    /// Points the tail at `x`, in the bubble's coordinates.
    pub fn set_tail_x(&self, x: f64) {
        let body = body_rect(self.bounds());
        let min = body.origin.x + 4.0 * PIXEL;
        let max = (body.origin.x + body.size.width - 8.0 * PIXEL).max(min);
        self.ivars().tail_x.set(snap(x.clamp(min, max)));
        self.setNeedsDisplay(true);
    }

    pub fn set_sidebar_right(&self, x: f64) {
        self.ivars().sidebar_right.set(x);
        self.setNeedsDisplay(true);
    }

    fn draw(&self) {
        let body = body_rect(self.bounds());
        let tail_x = self.ivars().tail_x.get();
        if style::is_classic() {
            return self.draw_classic(body, tail_x);
        }
        let shadow = offset(body, SHADOW);

        fill(&stepped(shadow), &theme::shadow());
        fill(&tail_outline(shadow, tail_x + SHADOW), &theme::shadow());
        fill(&stepped(body), &theme::outline());
        fill(&tail_outline(body, tail_x), &theme::outline());
        fill(&stepped(inset(body, PIXEL)), &theme::paper());
        self.draw_sidebar(body);
        fill(&tail_inside(body, tail_x), &theme::paper());
    }

    /// The classic style's bubble: rounded, a hairline outline, a smooth tail
    /// and a soft shadow, with the sidebar clipped to its left side.
    fn draw_classic(&self, body: NSRect, tail_x: f64) {
        let outline = classic_outline(body, tail_x);
        NSGraphicsContext::saveGraphicsState_class();
        let shadow = NSShadow::new();
        shadow.setShadowColor(Some(&theme::shadow().colorWithAlphaComponent(0.45)));
        shadow.setShadowBlurRadius(SHADOW);
        // Shadow offsets ignore the flip: negative y is down on screen.
        shadow.setShadowOffset(NSSize::new(1.0, -2.0));
        shadow.set();
        theme::paper().setFill();
        outline.fill();
        NSGraphicsContext::restoreGraphicsState_class();

        let right = self.ivars().sidebar_right.get();
        if right > 0.0 {
            NSGraphicsContext::saveGraphicsState_class();
            outline.addClip();
            theme::sidebar().setFill();
            let column = rect(
                body.origin.x,
                body.origin.y,
                right - body.origin.x,
                body.size.height,
            );
            NSBezierPath::fillRect(column);
            theme::divider().setFill();
            NSBezierPath::fillRect(rect(right, body.origin.y, 1.0, body.size.height));
            NSGraphicsContext::restoreGraphicsState_class();
        }
        theme::outline().setStroke();
        outline.setLineWidth(1.0);
        outline.stroke();
    }

    fn draw_sidebar(&self, body: NSRect) {
        let right = self.ivars().sidebar_right.get();
        if right <= 0.0 {
            return;
        }
        let inner = inset(body, PIXEL);
        let column = NSRect::new(
            inner.origin,
            NSSize::new(right - inner.origin.x, inner.size.height),
        );
        fill(&stepped_left(column), &theme::sidebar());
        let divider = rect(
            right,
            inner.origin.y + PIXEL,
            PIXEL,
            inner.size.height - 2.0 * PIXEL,
        );
        fill(&[divider], &theme::divider());
    }
}

/// The rounded box and its tail as one outline, for the classic style.
fn classic_outline(body: NSRect, tail_x: f64) -> Retained<NSBezierPath> {
    let r = inset(body, 0.5);
    let (left, top) = (r.origin.x, r.origin.y);
    let (right, bottom) = (left + r.size.width, top + r.size.height);
    let width = (TAIL_ROWS - 1) as f64 * PIXEL;
    let corner = |path: &NSBezierPath, at: (f64, f64), to: (f64, f64)| {
        path.appendBezierPathWithArcFromPoint_toPoint_radius(
            NSPoint::new(at.0, at.1),
            NSPoint::new(to.0, to.1),
            CLASSIC_RADIUS,
        );
    };
    // Clockwise on screen (the view is flipped), the tail cut into the bottom edge.
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(left + CLASSIC_RADIUS, top));
    corner(&path, (right, top), (right, bottom));
    corner(&path, (right, bottom), (left, bottom));
    path.lineToPoint(NSPoint::new(tail_x + width, bottom));
    path.lineToPoint(NSPoint::new(tail_x + 2.0, bottom + TAIL_HEIGHT - 1.0));
    path.lineToPoint(NSPoint::new(tail_x, bottom));
    corner(&path, (left, bottom), (left, top));
    corner(&path, (left, top), (right, top));
    path.closePath();
    path
}

/// The box without the tail and shadow.
fn body_rect(bounds: NSRect) -> NSRect {
    NSRect::new(
        bounds.origin,
        NSSize::new(
            bounds.size.width - SHADOW,
            bounds.size.height - SHADOW - TAIL_HEIGHT,
        ),
    )
}

/// A rectangle with two-step pixel corners, as three overlapping bands.
fn stepped(r: NSRect) -> [NSRect; 3] {
    let (x, y, w, h) = (r.origin.x, r.origin.y, r.size.width, r.size.height);
    [
        rect(x + 2.0 * PIXEL, y, w - 4.0 * PIXEL, h),
        rect(x + PIXEL, y + PIXEL, w - 2.0 * PIXEL, h - 2.0 * PIXEL),
        rect(x, y + 2.0 * PIXEL, w, h - 4.0 * PIXEL),
    ]
}

/// Like `stepped`, with square corners on the right (the sidebar column).
fn stepped_left(r: NSRect) -> [NSRect; 3] {
    let (x, y, w, h) = (r.origin.x, r.origin.y, r.size.width, r.size.height);
    [
        rect(x + 2.0 * PIXEL, y, w - 2.0 * PIXEL, h),
        rect(x + PIXEL, y + PIXEL, w - PIXEL, h - 2.0 * PIXEL),
        rect(x, y + 2.0 * PIXEL, w, h - 4.0 * PIXEL),
    ]
}

/// Tail rows under the body, narrowing to a point; each row is one pixel
/// wider than its inside on both sides.
fn tail_outline(body: NSRect, tail_x: f64) -> Vec<NSRect> {
    let bottom = body.origin.y + body.size.height;
    (0..TAIL_ROWS)
        .map(|row| {
            let inside = (TAIL_ROWS - 1 - row) as f64 * PIXEL;
            rect(
                tail_x - PIXEL,
                bottom + row as f64 * PIXEL,
                inside + 2.0 * PIXEL,
                PIXEL,
            )
        })
        .collect()
}

/// The tail's inside, plus the gap it opens in the body's bottom outline.
fn tail_inside(body: NSRect, tail_x: f64) -> Vec<NSRect> {
    let bottom = body.origin.y + body.size.height;
    let opening = rect(
        tail_x,
        bottom - PIXEL,
        (TAIL_ROWS - 1) as f64 * PIXEL,
        PIXEL,
    );
    let rows = (0..TAIL_ROWS - 1).map(|row| {
        let inside = (TAIL_ROWS - 1 - row) as f64 * PIXEL;
        rect(tail_x, bottom + row as f64 * PIXEL, inside, PIXEL)
    });
    std::iter::once(opening).chain(rows).collect()
}

fn fill(rects: &[NSRect], color: &NSColor) {
    color.setFill();
    for r in rects {
        NSBezierPath::fillRect(*r);
    }
}

fn inset(r: NSRect, by: f64) -> NSRect {
    rect(
        r.origin.x + by,
        r.origin.y + by,
        r.size.width - 2.0 * by,
        r.size.height - 2.0 * by,
    )
}

fn offset(r: NSRect, by: f64) -> NSRect {
    rect(
        r.origin.x + by,
        r.origin.y + by,
        r.size.width,
        r.size.height,
    )
}

fn snap(x: f64) -> f64 {
    (x / PIXEL).round() * PIXEL
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
