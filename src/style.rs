//! The app's look: pixel art (the default) or a classic one, with the system
//! font, a sober palette, rounded shapes and a smooth vector cat.
//!
//! Picked from the cat's menu and saved in the settings; it is read once at
//! launch, before any window is built, so changing it relaunches the app.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Pixel,
    Classic,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::Pixel, Style::Classic];

    pub fn name(self) -> &'static str {
        match self {
            Style::Pixel => "Pixel Art",
            Style::Classic => "Classic",
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Sets the look for this launch.
pub fn set(style: Style) {
    CURRENT.store(style as u8, Ordering::Relaxed);
}

/// The look of this launch.
pub fn current() -> Style {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Style::Classic,
        _ => Style::Pixel,
    }
}

pub fn is_classic() -> bool {
    current() == Style::Classic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_is_saved_lowercase_and_defaults_to_pixel() {
        assert_eq!(
            serde_json::to_string(&Style::Classic).unwrap(),
            "\"classic\""
        );
        assert_eq!(Style::default(), Style::Pixel);
    }
}
