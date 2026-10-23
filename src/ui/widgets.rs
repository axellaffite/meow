//! Controls in the pixel style: square text fields, flat block buttons and
//! pixel checkboxes.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSAttributedStringNSExtendedStringDrawing, NSAutoresizingMaskOptions, NSBezierPath, NSButton,
    NSCellImagePosition, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSEvent,
    NSFocusRingType, NSFont, NSImage, NSLayoutManager, NSStringDrawingOptions, NSText, NSTextField,
    NSTextFieldCell, NSView,
};
use objc2_foundation::{NSNotification, NSPoint, NSRange, NSRect, NSSize, NSString};

use meow::style;

use super::bubble;
use super::rich_text::{self, Attributes};
use super::theme;

/// Width of a field's pixel border.
const FIELD_BORDER: f64 = 3.0;
/// Text inset from a field's left and right edges, inside its border.
const FIELD_INSET: f64 = FIELD_BORDER + 10.0;
const CHECKBOX_SIZE: f64 = 18.0;
/// Corner radius of cards, fields and buttons in the classic style.
const CLASSIC_RADIUS: f64 = 6.0;
/// One "pixel" of the checkbox drawing.
const CHECKBOX_PIXEL: f64 = 3.0;

/// How a [`text_button`] is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Dark on gold: the page's main action.
    Primary,
    /// Light on dark with a grey border: secondary actions.
    Quiet,
    /// Dark on green: send or start something.
    Go,
    /// Dark on orange: asking Claude.
    Accent,
    /// Light on dark, outlined in orange.
    OutlineAccent,
    /// Light on dark, outlined in blue.
    OutlineLink,
    /// No fill: gold text, like a link.
    Link,
}

define_class!(
    /// A text field cell that keeps its text clear of the pixel border and
    /// centers it vertically.
    #[unsafe(super(NSTextFieldCell))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowPaddedCell"]
    pub struct PaddedCell;

    impl PaddedCell {
        #[unsafe(method(drawingRectForBounds:))]
        fn drawing_rect_for_bounds(&self, bounds: NSRect) -> NSRect {
            self.text_rect(bounds)
        }

        // The field editor is placed by these two, not by the drawing rect.
        #[unsafe(method(editWithFrame:inView:editor:delegate:event:))]
        fn edit_with_frame(
            &self,
            frame: NSRect,
            view: &NSView,
            editor: &NSText,
            delegate: Option<&AnyObject>,
            event: Option<&NSEvent>,
        ) {
            let frame = self.text_rect(frame);
            // SAFETY: forwards the same arguments to NSTextFieldCell, with the inset frame.
            unsafe {
                let _: () = msg_send![super(self), editWithFrame: frame, inView: view, editor: editor, delegate: delegate, event: event];
            }
        }

        #[unsafe(method(selectWithFrame:inView:editor:delegate:start:length:))]
        fn select_with_frame(
            &self,
            frame: NSRect,
            view: &NSView,
            editor: &NSText,
            delegate: Option<&AnyObject>,
            start: isize,
            length: isize,
        ) {
            let frame = self.text_rect(frame);
            // SAFETY: forwards the same arguments to NSTextFieldCell, with the inset frame.
            unsafe {
                let _: () = msg_send![super(self), selectWithFrame: frame, inView: view, editor: editor, delegate: delegate, start: start, length: length];
            }
        }
    }
);

impl PaddedCell {
    /// `bounds` inset from the sides, and from the top and bottom by what
    /// centers one line in a one-line field, so a taller field shows more lines.
    fn text_rect(&self, bounds: NSRect) -> NSRect {
        let line = self
            .font()
            .map_or(bounds.size.height, |font| line_height(&font));
        let dy = ((COMPOSER_FIELD_HEIGHT - line) / 2.0)
            .min((bounds.size.height - line) / 2.0)
            .max(0.0)
            .floor();
        NSRect::new(
            NSPoint::new(bounds.origin.x + FIELD_INSET, bounds.origin.y + dy),
            NSSize::new(
                (bounds.size.width - 2.0 * FIELD_INSET).max(0.0),
                bounds.size.height - 2.0 * dy,
            ),
        )
    }

    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSTextFieldCell's text initializer on a fresh allocation.
        unsafe { msg_send![super(this), initTextCell: &*NSString::from_str("")] }
    }
}

/// Most lines a field grows to before its text scrolls.
const MAX_FIELD_LINES: f64 = 3.0;
/// Extra height of a field showing more than one line.
const GROWN_SLACK: f64 = 6.0;

/// The height text system lines take in `font`, as the field editor lays them out.
fn line_height(font: &NSFont) -> f64 {
    NSLayoutManager::new().defaultLineHeightForFont(font).ceil()
}

#[derive(Default)]
pub struct GrowingFieldIvars {
    /// Views stacked on the field's bar, kept clear of it as it grows: each
    /// one's bottom edge moves up with the bar.
    above: RefCell<Vec<Retained<NSView>>>,
    /// Views sitting on the bar that move up with it, keeping their height.
    lifted: RefCell<Vec<Retained<NSView>>>,
}

define_class!(
    /// A text field that wraps and grows, with its bar, from one line to
    /// [`MAX_FIELD_LINES`] as text is typed; past that its text scrolls.
    #[unsafe(super(NSTextField))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowGrowingField"]
    #[ivars = GrowingFieldIvars]
    pub struct GrowingField;

    impl GrowingField {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            // SAFETY: forwards the notification to NSTextField's own handler.
            unsafe {
                let _: () = msg_send![super(self), textDidChange: notification];
            }
            self.fit();
        }
    }
);

impl GrowingField {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(GrowingFieldIvars::default());
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(100.0, COMPOSER_FIELD_HEIGHT),
        );
        // SAFETY: NSTextField's designated initializer, called once on a fresh allocation.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// How far the field has grown past one line.
    pub fn extra_height(&self) -> f64 {
        (self.frame().size.height - COMPOSER_FIELD_HEIGHT).max(0.0)
    }

    /// Grows or shrinks the field and its bar to the lines its text needs.
    pub fn fit(&self) {
        let Some(font) = self.font() else {
            return;
        };
        let line = line_height(&font);
        let width = (self.frame().size.width - 2.0 * FIELD_INSET).max(1.0);
        let text = rich_text::attributed(
            &self.stringValue().to_string(),
            &Attributes::plain(theme::ink(), font.clone()),
        );
        let used = text
            .boundingRectWithSize_options_context(
                NSSize::new(width, f64::MAX),
                NSStringDrawingOptions::UsesLineFragmentOrigin,
                None,
            )
            .size
            .height;
        let lines = (used / line).ceil().clamp(1.0, MAX_FIELD_LINES);
        // A little slack once grown, so the top line is never shaved off.
        let slack = if lines > 1.0 { GROWN_SLACK } else { 0.0 };
        let target = COMPOSER_FIELD_HEIGHT + (lines - 1.0) * line + slack;
        let delta = target - self.frame().size.height;
        if delta.abs() >= 0.5 {
            self.grow(delta);
        }
        self.fit_editor();
    }

    /// Keeps the text being typed inside the border: past the last line it
    /// scrolls rather than spilling below.
    fn fit_editor(&self) {
        let Some(editor) = self.currentEditor() else {
            return;
        };
        if let Some(cell) = self.cell() {
            let inside = cell.drawingRectForBounds(self.bounds());
            // SAFETY: a plain getter; the field editor sits in the field's clip view.
            if let Some(clip) = unsafe { editor.superview() } {
                clip.setFrame(inside);
            }
        }
        editor.scrollRangeToVisible(editor.selectedRange());
    }

    /// Grows the field and its bar by `delta`, and the views above out of the way.
    fn grow(&self, delta: f64) {
        // SAFETY: a plain getter; the field always sits in its composer bar.
        let Some(bar) = (unsafe { self.superview() }) else {
            return;
        };
        // The bar grows upward; autoresizing grows the field and keeps the
        // buttons at the bottom and the white line on top.
        let frame = bar.frame();
        bar.setFrameSize(NSSize::new(frame.size.width, frame.size.height + delta));
        for view in self.ivars().lifted.borrow().iter() {
            let frame = view.frame();
            view.setFrameOrigin(NSPoint::new(frame.origin.x, frame.origin.y + delta));
        }
        for view in self.ivars().above.borrow().iter() {
            let frame = view.frame();
            view.setFrame(NSRect::new(
                NSPoint::new(frame.origin.x, frame.origin.y + delta),
                NSSize::new(frame.size.width, (frame.size.height - delta).max(0.0)),
            ));
        }
    }

    /// Keeps `view`'s bottom edge clear of the bar as the field grows.
    pub fn keep_clear(&self, view: &NSView) {
        self.ivars().above.borrow_mut().push(view.retain());
    }

    /// Moves `view` up and down with the top of the bar, keeping its height.
    pub fn keep_above(&self, view: &NSView) {
        self.ivars().lifted.borrow_mut().push(view.retain());
    }

    /// Sets the text, caret at its end, and fits the field to it.
    pub fn set_text(&self, text: &str) {
        let text = NSString::from_str(text);
        self.setStringValue(&text);
        if let Some(editor) = self.currentEditor() {
            editor.setString(&text);
            editor.setSelectedRange(NSRange::new(text.length(), 0));
        }
        self.fit();
    }
}

/// Paints `view` as a flat block: a `fill` and an inner `border` (color, width).
pub fn paint(view: &NSView, fill: Option<&NSColor>, border: Option<(&NSColor, f64)>) {
    view.setWantsLayer(true);
    let Some(layer) = view.layer() else {
        return;
    };
    // The classic style rounds the corners (less on thin strips) and draws hairline borders.
    let classic = style::is_classic();
    let radius = if classic {
        let size = view.frame().size;
        CLASSIC_RADIUS.min(size.width.min(size.height) / 2.0)
    } else {
        0.0
    };
    layer.setCornerRadius(radius);
    layer.setBackgroundColor(fill.map(|color| color.CGColor()).as_deref());
    match border {
        Some((color, width)) => {
            layer.setBorderColor(Some(&color.CGColor()));
            layer.setBorderWidth(if classic { 1.0 } else { width });
        }
        None => layer.setBorderWidth(0.0),
    }
}

/// A square input field: pixel font on a dark fill, inside a grey pixel border.
pub fn text_field(mtm: MainThreadMarker, placeholder: &str) -> Retained<GrowingField> {
    let field = GrowingField::new(mtm);
    let cell = PaddedCell::new(mtm);
    cell.setEditable(true);
    cell.setSelectable(true);
    // Wraps onto more lines as the field grows; ⌥ or ⇧ Return adds a line break.
    cell.setWraps(true);
    cell.setScrollable(false);
    cell.setUsesSingleLineMode(false);
    field.setCell(Some(&cell));
    field.setFont(Some(&theme::font(theme::BODY_SIZE)));
    field.setTextColor(Some(&theme::ink()));
    field.setBezeled(false);
    field.setBordered(false);
    field.setDrawsBackground(false);
    field.setFocusRingType(NSFocusRingType::None);
    set_placeholder(&field, placeholder);
    paint(
        &field,
        Some(&theme::field_background()),
        Some((&theme::field_border(), FIELD_BORDER)),
    );
    field
}

/// Changes the muted hint a [`text_field`] shows while empty.
pub fn set_placeholder(field: &NSTextField, placeholder: &str) {
    field.setPlaceholderAttributedString(Some(&rich_text::attributed(
        placeholder,
        &Attributes::plain(theme::muted(), theme::font(theme::BODY_SIZE)),
    )));
}

/// A flat block button reading `LABEL` in the label font.
pub fn text_button(
    mtm: MainThreadMarker,
    label: &str,
    style: ButtonStyle,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    // SAFETY: callers pass the app-lifetime controller and one of its actions.
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(""),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setBordered(false);
    restyle_button(&button, label, style);
    button
}

/// Repaints a [`text_button`] in `style` with `label` (a toggle changing state).
pub fn restyle_button(button: &NSButton, label: &str, style: ButtonStyle) {
    let (fill, border) = match style {
        ButtonStyle::Primary => (theme::highlight(), None),
        ButtonStyle::Go => (theme::live(), None),
        ButtonStyle::Accent => (theme::accent(), None),
        ButtonStyle::Quiet => (theme::sidebar(), Some(theme::field_border())),
        ButtonStyle::OutlineAccent => (theme::sidebar(), Some(theme::accent())),
        ButtonStyle::OutlineLink => (theme::sidebar(), Some(theme::link())),
        ButtonStyle::Link => (theme::sidebar(), None),
    };
    let fill = (style != ButtonStyle::Link).then_some(fill);
    paint(
        button,
        fill.as_deref(),
        border.as_deref().map(|color| (color, 2.0)),
    );
    set_text_button_label(button, label, style);
}

/// Inner padding of a [`composer`] bar.
const COMPOSER_PADDING: f64 = 12.0;
/// The white line along a composer's top edge.
const COMPOSER_LINE: f64 = 3.0;
const COMPOSER_FIELD_HEIGHT: f64 = 36.0;
const COMPOSER_GAP: f64 = 8.0;
/// The darker line under a filled button, like a pixel bevel.
const BUTTON_UNDERLINE: f64 = 3.0;

/// Height of a one-row [`composer`] bar.
pub const COMPOSER_HEIGHT: f64 = 2.0 * COMPOSER_PADDING + COMPOSER_FIELD_HEIGHT + COMPOSER_LINE;
/// Height of a [`composer`] bar with its buttons on a row of their own.
pub const STACKED_COMPOSER_HEIGHT: f64 = COMPOSER_HEIGHT + COMPOSER_GAP + COMPOSER_FIELD_HEIGHT;

/// A button laid out by [`composer`]: the button, its width and its style.
pub type ComposerButton<'a> = (&'a NSButton, f64, ButtonStyle);

/// Lays out `field` and buttons in a dark bar along the bottom of `page`,
/// under a white line, across the page's whole width. With no `left` buttons,
/// `right` sits after the field on one row; otherwise the field gets the whole
/// first row, and the buttons a second one, `left` against the left edge and
/// `right` against the right. Filled buttons get a darker line beneath them.
/// Returns the bar's height inside the page.
pub fn composer(
    mtm: MainThreadMarker,
    page: &NSView,
    field: &NSTextField,
    left: &[ComposerButton],
    right: &[ComposerButton],
) -> f64 {
    let stacked = !left.is_empty();
    let height = if stacked {
        STACKED_COMPOSER_HEIGHT
    } else {
        COMPOSER_HEIGHT
    };
    let width = page.frame().size.width;
    // The page reaches the bubble's outline, so the bar does too.
    let bar_width = width;
    let bar_height = height;
    let (bar_y, stick) = if page.isFlipped() {
        (
            page.frame().size.height - height,
            NSAutoresizingMaskOptions::ViewMinYMargin,
        )
    } else {
        (0.0, NSAutoresizingMaskOptions::ViewMaxYMargin)
    };
    let bar = NSView::initWithFrame(NSView::alloc(mtm), frame(0.0, bar_y, bar_width, bar_height));
    bar.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | stick);
    paint(&bar, Some(&theme::sidebar()), None);
    page.addSubview(&bar);

    // The bar is not flipped: y grows upward from its bottom edge.
    let line = NSView::initWithFrame(
        NSView::alloc(mtm),
        frame(0.0, bar_height - COMPOSER_LINE, bar_width, COMPOSER_LINE),
    );
    // Stays on top as the bar grows with its field.
    line.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    paint(&line, Some(&theme::outline()), None);
    bar.addSubview(&line);

    let bottom = COMPOSER_PADDING;
    let field_y = if stacked {
        bottom + COMPOSER_FIELD_HEIGHT + COMPOSER_GAP
    } else {
        bottom
    };
    let inner_right = width - COMPOSER_PADDING - bubble::PADDING;
    let mut x = inner_right;
    for (button, button_width, style) in right.iter().rev() {
        x -= button_width;
        place_button(
            mtm,
            &bar,
            button,
            *style,
            frame(x, bottom, *button_width, COMPOSER_FIELD_HEIGHT),
            true,
        );
        x -= COMPOSER_GAP;
    }
    let mut left_x = COMPOSER_PADDING;
    for (button, button_width, style) in left {
        place_button(
            mtm,
            &bar,
            button,
            *style,
            frame(left_x, bottom, *button_width, COMPOSER_FIELD_HEIGHT),
            false,
        );
        left_x += button_width + COMPOSER_GAP;
    }
    let field_right = if stacked { inner_right } else { x };
    field.setFrame(frame(
        COMPOSER_PADDING,
        field_y,
        (field_right - COMPOSER_PADDING).max(0.0),
        COMPOSER_FIELD_HEIGHT,
    ));
    field.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    bar.addSubview(field);
    height
}

/// Adds `button` to `bar` at `rect`, with its darker underline when filled.
/// Buttons `on_right` keep to the bar's right edge when it widens.
fn place_button(
    mtm: MainThreadMarker,
    bar: &NSView,
    button: &NSButton,
    style: ButtonStyle,
    rect: NSRect,
    on_right: bool,
) {
    // Buttons keep to the bottom as the bar grows with its field.
    let mask = NSAutoresizingMaskOptions::ViewMaxYMargin
        | if on_right {
            NSAutoresizingMaskOptions::ViewMinXMargin
        } else {
            NSAutoresizingMaskOptions::ViewMaxXMargin
        };
    button.setFrame(rect);
    button.setAutoresizingMask(mask);
    bar.addSubview(button);
    if let Some(shade) = underline_color(style) {
        let under = NSView::initWithFrame(
            NSView::alloc(mtm),
            frame(
                rect.origin.x,
                rect.origin.y - BUTTON_UNDERLINE,
                rect.size.width,
                BUTTON_UNDERLINE,
            ),
        );
        under.setAutoresizingMask(mask);
        paint(&under, Some(&shade), None);
        bar.addSubview(&under);
    }
}

fn underline_color(style: ButtonStyle) -> Option<Retained<NSColor>> {
    // The bevel is a pixel-art touch.
    if style::is_classic() {
        return None;
    }
    match style {
        ButtonStyle::Primary | ButtonStyle::Accent => Some(theme::primary_shade()),
        ButtonStyle::Go => Some(theme::live_shade()),
        ButtonStyle::Quiet
        | ButtonStyle::OutlineAccent
        | ButtonStyle::OutlineLink
        | ButtonStyle::Link => None,
    }
}

fn frame(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

/// Changes the label of a [`text_button`] of `style`.
pub fn set_text_button_label(button: &NSButton, label: &str, style: ButtonStyle) {
    let color = match style {
        ButtonStyle::Primary | ButtonStyle::Go | ButtonStyle::Accent => theme::on_accent(),
        ButtonStyle::Quiet | ButtonStyle::OutlineAccent | ButtonStyle::OutlineLink => theme::ink(),
        ButtonStyle::Link => theme::highlight(),
    };
    // Silkscreen has capitals only; the classic style keeps the label as written.
    let label = if style::is_classic() {
        label.to_string()
    } else {
        label.to_uppercase()
    };
    button.setAttributedTitle(&rich_text::attributed(
        &label,
        &Attributes::plain(color, theme::label_font(theme::LABEL_SIZE)),
    ));
}

/// A pixel checkbox: a grey-bordered dark square, green with a dark ring when on.
pub fn checkbox(
    mtm: MainThreadMarker,
    target: &AnyObject,
    action: Sel,
    on: bool,
) -> Retained<NSButton> {
    // SAFETY: callers pass the app-lifetime controller and one of its actions.
    let button = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(""),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setBordered(false);
    button.setImage(Some(&checkbox_image(false)));
    button.setAlternateImage(Some(&checkbox_image(true)));
    button.setImagePosition(NSCellImagePosition::ImageOnly);
    button.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    button
}

fn checkbox_image(on: bool) -> Retained<NSImage> {
    let size = NSSize::new(CHECKBOX_SIZE, CHECKBOX_SIZE);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    let square = |inset: f64, color: &NSColor| {
        color.setFill();
        let side = CHECKBOX_SIZE - 2.0 * inset;
        NSBezierPath::fillRect(NSRect::new(
            NSPoint::new(inset, inset),
            NSSize::new(side, side),
        ));
    };
    #[allow(deprecated)]
    image.lockFocus();
    if style::is_classic() {
        classic_checkbox(on);
    } else if on {
        square(0.0, &theme::live());
        square(CHECKBOX_PIXEL, &theme::sidebar());
        square(2.0 * CHECKBOX_PIXEL, &theme::live());
    } else {
        square(0.0, &theme::muted());
        square(CHECKBOX_PIXEL, &theme::sidebar());
    }
    #[allow(deprecated)]
    image.unlockFocus();
    image
}

/// A rounded box with a hairline border, filled with a tick mark when on.
fn classic_checkbox(on: bool) {
    let inset = 1.0;
    let side = CHECKBOX_SIZE - 2.0 * inset;
    let rect = NSRect::new(NSPoint::new(inset, inset), NSSize::new(side, side));
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, 4.0, 4.0);
    if on {
        theme::live().setFill();
        path.fill();
        let tick = NSBezierPath::bezierPath();
        // The image is not flipped: y grows upward.
        tick.moveToPoint(NSPoint::new(5.0, 9.0));
        tick.lineToPoint(NSPoint::new(8.0, 5.5));
        tick.lineToPoint(NSPoint::new(13.5, 12.5));
        tick.setLineWidth(2.0);
        theme::on_accent().setStroke();
        tick.stroke();
    } else {
        theme::field_background().setFill();
        path.fill();
        theme::muted().setStroke();
        path.setLineWidth(1.0);
        path.stroke();
    }
}

/// `frame` drawn with each pixel `scale` points square; transparent pixels stay clear.
pub fn pixel_image(frame: &meow::sprites::Frame, scale: f64) -> Retained<NSImage> {
    let image = NSImage::initWithSize(
        NSImage::alloc(),
        NSSize::new(frame.width as f64 * scale, frame.height as f64 * scale),
    );
    #[allow(deprecated)]
    image.lockFocus();
    for y in 0..frame.height {
        for x in 0..frame.width {
            let Some([r, g, b]) = frame.get(x, y) else {
                continue;
            };
            let channel = |v: u8| f64::from(v) / 255.0;
            NSColor::colorWithSRGBRed_green_blue_alpha(channel(r), channel(g), channel(b), 1.0)
                .setFill();
            // Image space grows upward; frame rows go down.
            let origin = NSPoint::new(x as f64 * scale, (frame.height - 1 - y) as f64 * scale);
            NSBezierPath::fillRect(NSRect::new(origin, NSSize::new(scale, scale)));
        }
    }
    #[allow(deprecated)]
    image.unlockFocus();
    image
}
