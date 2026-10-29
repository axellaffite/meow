//! How Meow looks, picked from the cat's menu: the coat the cat wears (a
//! built-in one or a theme from a photo of the user's cat), the size the
//! chat bubble opens at, the style and whether news shows in a bubble over
//! the cat. Saved among the settings (see
//! `settings::Store`).

use serde::{Deserialize, Serialize};

use crate::cat_photo::CustomCoat;
use crate::sprites::Coat;
use crate::style::Style;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub coat: Coat,
    /// Themes made from photos of the user's cats, in the order added.
    pub custom_coats: Vec<CustomCoat>,
    /// The chat bubble's size when it opens.
    pub window_size: WindowSize,
    /// Pixel art or classic; read at launch.
    pub style: Style,
    /// Unread news shows only the cat's `!`, not the bubble of where it
    /// comes from.
    pub hide_news_bubble: bool,
}

/// How big the chat bubble opens; it can still be resized by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowSize {
    #[default]
    Standard,
    Large,
    Huge,
}

impl WindowSize {
    pub const ALL: [WindowSize; 3] = [WindowSize::Standard, WindowSize::Large, WindowSize::Huge];

    pub fn name(self) -> &'static str {
        match self {
            WindowSize::Standard => "Standard",
            WindowSize::Large => "Large",
            WindowSize::Huge => "Extra Large",
        }
    }

    /// Width and height in points, before fitting the screen.
    pub fn points(self) -> (f64, f64) {
        match self {
            WindowSize::Standard => (620.0, 470.0),
            WindowSize::Large => (860.0, 640.0),
            WindowSize::Huge => (1120.0, 820.0),
        }
    }
}

impl Appearance {
    /// Every theme to pick from, built-in ones first, with their names.
    pub fn coats(&self) -> Vec<(String, Coat)> {
        Coat::ALL
            .iter()
            .map(|coat| (coat.name().to_string(), *coat))
            .chain(self.custom_coats.iter().map(|c| (c.name.clone(), c.coat())))
            .collect()
    }

    /// The custom theme being worn, if any.
    pub fn current_custom(&self) -> Option<&CustomCoat> {
        self.custom_coats.iter().find(|c| c.coat() == self.coat)
    }

    /// With `custom` added and worn; it replaces a theme of the same name.
    pub fn with_custom(&self, custom: CustomCoat) -> Self {
        let custom_coats = self
            .custom_coats
            .iter()
            .filter(|c| !c.name.eq_ignore_ascii_case(&custom.name))
            .cloned()
            .chain([custom.clone()])
            .collect();
        Self {
            coat: custom.coat(),
            custom_coats,
            ..self.clone()
        }
    }

    /// Without the custom theme being worn, back in the default coat.
    pub fn without_current_custom(&self) -> Self {
        let Some(current) = self.current_custom() else {
            return self.clone();
        };
        Self {
            coat: Coat::default(),
            custom_coats: self
                .custom_coats
                .iter()
                .filter(|c| *c != current)
                .cloned()
                .collect(),
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprites::Palette;

    fn mochi() -> CustomCoat {
        custom("Mochi", [20, 20, 20])
    }

    fn custom(name: &str, fur: [u8; 3]) -> CustomCoat {
        CustomCoat {
            name: name.to_string(),
            palette: Palette {
                fur,
                stripe: fur,
                blaze: [255, 255, 255],
                muzzle: [255, 255, 255],
                legs: [255, 255, 255],
                tail: fur,
                near_eye: [60, 140, 60],
                far_eye: [60, 140, 60],
            },
        }
    }

    #[test]
    fn a_new_custom_theme_is_listed_and_worn() {
        let appearance = Appearance::default().with_custom(mochi());
        assert_eq!(appearance.coat, mochi().coat());
        assert_eq!(appearance.current_custom(), Some(&mochi()));
        let names: Vec<_> = appearance
            .coats()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            names,
            ["Ginger", "Willow", "Alfie", "Dédé", "Corgi", "Mochi"]
        );
    }

    #[test]
    fn a_theme_of_the_same_name_replaces_the_old_one() {
        let newer = custom("mochi", [40, 40, 40]);
        let appearance = Appearance::default()
            .with_custom(mochi())
            .with_custom(custom("Pixel", [200, 120, 60]))
            .with_custom(newer.clone());
        let names: Vec<_> = appearance
            .custom_coats
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Pixel", "mochi"]);
        assert_eq!(appearance.coat, newer.coat());
    }

    #[test]
    fn removing_the_worn_theme_goes_back_to_ginger() {
        let appearance = Appearance::default()
            .with_custom(custom("Pixel", [200, 120, 60]))
            .with_custom(mochi())
            .without_current_custom();
        assert_eq!(appearance.coat, Coat::Ginger);
        assert_eq!(appearance.custom_coats.len(), 1);
        assert_eq!(appearance.custom_coats[0].name, "Pixel");
        let built_in = Appearance::default();
        assert_eq!(built_in.without_current_custom(), built_in);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let appearance: Appearance = serde_json::from_str("{}").unwrap();
        assert_eq!(appearance.coat, Coat::Ginger);
        assert_eq!(appearance.window_size, WindowSize::Standard);
        assert_eq!(appearance.style, Style::Pixel);
        let willow: Appearance = serde_json::from_str(r#"{"coat":"Willow"}"#).unwrap();
        assert_eq!(willow.coat, Coat::Willow);
    }
}
