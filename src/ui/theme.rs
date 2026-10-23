//! The pixel-art look: a dark 8-bit palette (Sweetie 16) and the bundled
//! pixel fonts.
//!
//! The fonts are VT323 (body), Silkscreen (labels) and Press Start 2P
//! (titles), all under the SIL Open Font License (see assets/fonts), embedded
//! in the binary.

use std::cell::RefCell;

use meow::style;
use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSFont, NSFontWeightSemibold};
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_text::{CTFont, CTFontDescriptor, CTFontManagerCreateFontDescriptorFromData};

const BODY_FONT: &str = "VT323";
const LABEL_FONT: &str = "Silkscreen";
const TITLE_FONT: &str = "PressStart2P";

const FONTS: [(&str, &[u8]); 3] = [
    (
        BODY_FONT,
        include_bytes!("../../assets/fonts/VT323-400.ttf"),
    ),
    (
        LABEL_FONT,
        include_bytes!("../../assets/fonts/Silkscreen-Regular.ttf"),
    ),
    (
        TITLE_FONT,
        include_bytes!("../../assets/fonts/PressStart2P-Regular.ttf"),
    ),
];

/// VT323 is drawn small for its point size, so sizes run larger than usual.
pub const BODY_SIZE: f64 = 18.0;
pub const SMALL_SIZE: f64 = 16.0;
/// Silkscreen: page names, section headings, button labels.
pub const LABEL_SIZE: f64 = 12.0;
/// Press Start 2P is wide and tall for its size.
pub const TITLE_SIZE: f64 = 12.0;

thread_local! {
    /// The bundled fonts, loaded straight from the binary; no system registration needed.
    static LOADED: RefCell<Vec<(&'static str, CFRetained<CTFontDescriptor>)>> =
        const { RefCell::new(Vec::new()) };
}

/// Loads the bundled fonts. Text falls back to the system font for any that fail.
pub fn register_fonts() -> Result<(), String> {
    let mut missing = Vec::new();
    LOADED.with(|loaded| {
        let mut loaded = loaded.borrow_mut();
        loaded.clear();
        for (name, bytes) in FONTS {
            // SAFETY: the data is a complete font file embedded in the binary.
            match unsafe { CTFontManagerCreateFontDescriptorFromData(&CFData::from_bytes(bytes)) } {
                Some(descriptor) => loaded.push((name, descriptor)),
                None => missing.push(name),
            }
        }
    });
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Could not load the pixel fonts ({}).",
            missing.join(", ")
        ))
    }
}

fn named(name: &str, size: f64) -> Option<Retained<NSFont>> {
    LOADED.with(|loaded| {
        let loaded = loaded.borrow();
        let (_, descriptor) = loaded
            .iter()
            .find(|(loaded_name, _)| *loaded_name == name)?;
        // SAFETY: a valid descriptor; a null matrix means the identity transform.
        let font = unsafe { CTFont::with_font_descriptor(descriptor, size, std::ptr::null()) };
        // SAFETY: CTFont is toll-free bridged to NSFont, and ownership moves with the pointer.
        unsafe { Retained::from_raw(CFRetained::into_raw(font).as_ptr().cast::<NSFont>()) }
    })
}

fn named_or_fallback(name: &str, size: f64) -> Retained<NSFont> {
    named(name, size).unwrap_or_else(|| {
        NSFont::userFixedPitchFontOfSize(size).unwrap_or_else(|| NSFont::systemFontOfSize(size))
    })
}

/// The body font. Bold text is drawn with a stroke (see `Attributes::heavy`).
/// In the classic style, the system font, a little smaller: VT323 is drawn
/// small for its point size.
pub fn font(size: f64) -> Retained<NSFont> {
    if style::is_classic() {
        return NSFont::systemFontOfSize(classic_body_size(size));
    }
    named_or_fallback(BODY_FONT, size)
}

fn classic_body_size(size: f64) -> f64 {
    (size * 0.78).round()
}

/// The label font: blocky capitals for page names, headings and buttons; in
/// the classic style, the system font in semibold.
pub fn label_font(size: f64) -> Retained<NSFont> {
    if style::is_classic() {
        // SAFETY: AppKit's font weight constants are immutable statics.
        let semibold = unsafe { NSFontWeightSemibold };
        return NSFont::systemFontOfSize_weight(size + 1.0, semibold);
    }
    named_or_fallback(LABEL_FONT, size)
}

/// The title font, for the MEOW wordmark and page titles; in the classic
/// style, the system font in bold.
pub fn title_font(size: f64) -> Retained<NSFont> {
    if style::is_classic() {
        return NSFont::boldSystemFontOfSize((size * 1.25).round());
    }
    named_or_fallback(TITLE_FONT, size)
}

/// The pixel-art color, or the classic one in the classic style.
fn pick(pixel: u32, classic: u32) -> Retained<NSColor> {
    rgb(if style::is_classic() { classic } else { pixel })
}

fn rgb(hex: u32) -> Retained<NSColor> {
    let channel = |shift: u32| f64::from((hex >> shift) & 0xff) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(channel(16), channel(8), channel(0), 1.0)
}

/// Main text: off-white on the dark panels.
pub fn ink() -> Retained<NSColor> {
    pick(0xf4f4f4, 0xf2f2f5)
}

/// Text drawn on the light fills (selected page, buttons).
pub fn on_accent() -> Retained<NSColor> {
    pick(0x1a1c2c, 0xffffff)
}

pub fn muted() -> Retained<NSColor> {
    pick(0x94b0c2, 0xa1a1aa)
}

pub fn faint() -> Retained<NSColor> {
    pick(0x7c8ba3, 0x80808a)
}

/// The bubble's pixel outline.
pub fn outline() -> Retained<NSColor> {
    pick(0xf4f4f4, 0x3a3a40)
}

/// Bubble fill.
pub fn paper() -> Retained<NSColor> {
    pick(0x333c57, 0x1f1f23)
}

pub fn sidebar() -> Retained<NSColor> {
    pick(0x1a1c2c, 0x18181b)
}

pub fn divider() -> Retained<NSColor> {
    pick(0xf4f4f4, 0x34343a)
}

/// The cat's orange fur.
pub fn accent() -> Retained<NSColor> {
    pick(0xef7d57, 0x4c8dff)
}

/// Gold: page titles, section headings and the main buttons.
pub fn highlight() -> Retained<NSColor> {
    pick(0xffcd75, 0x4c8dff)
}

/// The darker line under a gold button.
pub fn primary_shade() -> Retained<NSColor> {
    pick(0xb13e53, 0x2f6fe0)
}

/// The darker line under a green button.
pub fn live_shade() -> Retained<NSColor> {
    pick(0x38b764, 0x248a4e)
}

/// Background of the user's own messages.
pub fn user_bubble() -> Retained<NSColor> {
    pick(0x3b5dc9, 0x2c4a80)
}

/// Cyan: one of the Slack avatar colors.
pub fn cyan() -> Retained<NSColor> {
    pick(0x73eff7, 0x5ac8fa)
}

pub fn link() -> Retained<NSColor> {
    pick(0x41a6f6, 0x6aa8ff)
}

pub fn error() -> Retained<NSColor> {
    pick(0xff7b7b, 0xff6b6b)
}

/// Emphasis: italic text is tinted.
pub fn emphasis() -> Retained<NSColor> {
    pick(0xffcd75, 0xe4e4e7)
}

pub fn code_background() -> Retained<NSColor> {
    pick(0x1a1c2c, 0x141417)
}

pub fn field_background() -> Retained<NSColor> {
    pick(0x262b44, 0x2a2a2f)
}

/// Background of a list card (a task, an agenda item).
pub fn card() -> Retained<NSColor> {
    pick(0x262b44, 0x27272c)
}

/// A done card: lighter, so it recedes from the open ones.
pub fn card_done() -> Retained<NSColor> {
    pick(0x3e4866, 0x1f1f23)
}

/// Pixel border of fields and secondary buttons.
pub fn field_border() -> Retained<NSColor> {
    pick(0x566c86, 0x45454d)
}

/// Green: live status, send buttons, ticked checkboxes.
pub fn live() -> Retained<NSColor> {
    pick(0xa7f070, 0x34c759)
}

/// The bubble's hard drop shadow.
pub fn shadow() -> Retained<NSColor> {
    pick(0x0b0c16, 0x000000)
}
