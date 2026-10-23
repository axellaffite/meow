//! Choices the user makes from the cat's menu, kept across launches.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cat_photo::CustomCoat;
use crate::sprites::Coat;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub coat: Coat,
    /// Themes made from photos of the user's cats, in the order added.
    pub custom_coats: Vec<CustomCoat>,
    /// Slack drafts follow the user's learned style guide.
    pub write_as_me: bool,
    /// The chat bubble's size when it opens.
    pub window_size: WindowSize,
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

impl Settings {
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

    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("Could not read settings from {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Could not read {}: {e}", path.display())),
        }
    }

    /// Writes atomically, so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let fail = |e: &dyn std::fmt::Display| {
            format!("Could not save settings to {}: {e}", path.display())
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| fail(&e))?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, json).map_err(|e| fail(&e))?;
        std::fs::rename(&temp, path).map_err(|e| fail(&e))
    }
}

/// `~/Library/Application Support/Meow/settings.json`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/Meow/settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprites::Palette;

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("meow-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let settings = Settings {
            coat: Coat::Alfie,
            write_as_me: true,
            ..Settings::default()
        }
        .with_custom(mochi());
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert_eq!(
            Settings::load(&dir.join("missing.json")).unwrap(),
            Settings::default()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

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
        let settings = Settings::default().with_custom(mochi());
        assert_eq!(settings.coat, mochi().coat());
        assert_eq!(settings.current_custom(), Some(&mochi()));
        let names: Vec<_> = settings.coats().into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["Ginger", "Willow", "Alfie", "Dédé", "Mochi"]);
    }

    #[test]
    fn a_theme_of_the_same_name_replaces_the_old_one() {
        let newer = custom("mochi", [40, 40, 40]);
        let settings = Settings::default()
            .with_custom(mochi())
            .with_custom(custom("Pixel", [200, 120, 60]))
            .with_custom(newer.clone());
        let names: Vec<_> = settings
            .custom_coats
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Pixel", "mochi"]);
        assert_eq!(settings.coat, newer.coat());
    }

    #[test]
    fn removing_the_worn_theme_goes_back_to_ginger() {
        let settings = Settings::default()
            .with_custom(custom("Pixel", [200, 120, 60]))
            .with_custom(mochi())
            .without_current_custom();
        assert_eq!(settings.coat, Coat::Ginger);
        assert_eq!(settings.custom_coats.len(), 1);
        assert_eq!(settings.custom_coats[0].name, "Pixel");
        let built_in = Settings::default();
        assert_eq!(built_in.without_current_custom(), built_in);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.coat, Coat::Ginger);
        assert!(!settings.write_as_me);
        let willow: Settings = serde_json::from_str(r#"{"coat":"Willow"}"#).unwrap();
        assert_eq!(willow.coat, Coat::Willow);
    }
}
