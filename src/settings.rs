//! Choices the user makes, kept across launches in one JSON file.
//!
//! The file is one flat object, and each part of the app owns some of its
//! fields: it reads them into its own struct with `Store::get` and writes them
//! back with `Store::set`, which leaves every other field as it was. So a
//! feature keeps its settings in its own type, and two of them saving never
//! undo each other's changes.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The settings file, as read at launch.
#[derive(Debug, Default)]
pub struct Store {
    /// Where to save; `None` when the file could not be read (so it is never
    /// overwritten) or there is no home folder.
    path: Option<PathBuf>,
    values: Map<String, Value>,
}

impl Store {
    /// Reads the file at `path`; a missing file is an empty one.
    pub fn load(path: &Path) -> Result<Self, String> {
        let values = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("Could not read settings from {}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
        };
        Ok(Self {
            path: Some(path.to_path_buf()),
            values,
        })
    }

    /// Settings kept for this launch only.
    pub fn unsaved() -> Self {
        Self::default()
    }

    /// The fields of `T` as saved; missing ones take their default (`T` is
    /// `#[serde(default)]`). When one cannot be read, nothing is saved from
    /// then on, so the file is never overwritten with defaults.
    pub fn get<T: DeserializeOwned + Default>(&mut self) -> Result<T, String> {
        serde_json::from_value(Value::Object(self.values.clone())).map_err(|e| {
            let error = match self.path.take() {
                Some(path) => format!("Could not read settings from {}: {e}", path.display()),
                None => format!("Could not read settings: {e}"),
            };
            eprintln!("meow: {error}");
            error
        })
    }

    /// Saves the fields of `value`, keeping all the others.
    pub fn set<T: Serialize>(&mut self, value: &T) -> Result<(), String> {
        let Ok(Value::Object(fields)) = serde_json::to_value(value) else {
            return Err("Could not save settings: not a set of fields".to_string());
        };
        self.values.extend(fields);
        match &self.path {
            Some(path) => save(path, &self.values),
            None => Ok(()),
        }
    }
}

/// Writes atomically, so a crash never leaves a half-written file.
fn save(path: &Path, values: &Map<String, Value>) -> Result<(), String> {
    let fail =
        |e: &dyn std::fmt::Display| format!("Could not save settings to {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
    }
    let json = serde_json::to_string_pretty(values).map_err(|e| fail(&e))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json).map_err(|e| fail(&e))?;
    std::fs::rename(&temp, path).map_err(|e| fail(&e))
}

/// `~/Library/Application Support/Meow/settings.json`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/Meow/settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// One owner's fields.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(default)]
    struct Settings {
        write_as_me: bool,
        autopilot: bool,
    }

    /// Another owner's fields, as a feature would keep them.
    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Other {
        coat: String,
        size: u32,
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meow-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn saved_fields_are_read_back_after_a_relaunch() {
        let dir = temp_dir("round-trip");
        let path = dir.join("settings.json");
        let mut store = Store::load(&path).unwrap();
        let settings = Settings {
            write_as_me: true,
            autopilot: false,
        };
        store.set(&settings).unwrap();
        assert_eq!(
            Store::load(&path).unwrap().get::<Settings>().unwrap(),
            settings
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_one_owner_keeps_the_fields_of_the_others() {
        let dir = temp_dir("owners");
        let path = dir.join("settings.json");
        let mut store = Store::load(&path).unwrap();
        let other = Other {
            coat: "Willow".to_string(),
            size: 2,
        };
        store.set(&other).unwrap();
        store
            .set(&Settings {
                write_as_me: true,
                autopilot: true,
            })
            .unwrap();
        let reloaded = Store::load(&path).unwrap();
        let mut reloaded = reloaded;
        assert_eq!(reloaded.get::<Other>().unwrap(), other);
        assert!(reloaded.get::<Settings>().unwrap().autopilot);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fields_no_one_reads_any_more_are_kept() {
        let dir = temp_dir("unknown");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"future": [1, 2], "autopilot": true}"#).unwrap();
        let mut store = Store::load(&path).unwrap();
        store.set(&Settings::default()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"future\""), "{text}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_file_reads_as_defaults() {
        let dir = temp_dir("missing");
        let mut store = Store::load(&dir.join("settings.json")).unwrap();
        assert_eq!(store.get::<Settings>().unwrap(), Settings::default());
        assert!(!dir.exists(), "nothing written until something is set");
    }

    #[test]
    fn a_file_that_is_not_an_object_is_refused() {
        let dir = temp_dir("bad");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "[1, 2").unwrap();
        let error = Store::load(&path).unwrap_err();
        assert!(error.starts_with("Could not read settings from"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_field_that_cannot_be_read_is_reported_and_nothing_is_saved_after() {
        let dir = temp_dir("wrong-type");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        let saved = r#"{"autopilot": "yes", "coat": "Alfie"}"#;
        std::fs::write(&path, saved).unwrap();
        let mut store = Store::load(&path).unwrap();
        let error = store.get::<Settings>().unwrap_err();
        assert!(error.starts_with("Could not read settings from"), "{error}");
        assert_eq!(store.get::<Other>().unwrap().coat, "Alfie");
        store.set(&Settings::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unsaved_settings_are_kept_in_memory_only() {
        let mut store = Store::unsaved();
        store
            .set(&Settings {
                write_as_me: true,
                autopilot: false,
            })
            .unwrap();
        assert!(store.get::<Settings>().unwrap().write_as_me);
    }
}
