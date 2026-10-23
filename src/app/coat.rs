//! The cat's coat, picked from its menu and remembered across launches, and
//! themes made from a photo of the user's cat: the photo is shrunk in the
//! background, then a run that may only read that copy reports its colors.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use meow::cat_photo::{self, CustomCoat};
use meow::claude::{ClaudeEvent, Run};
use meow::conversations::ChatId;
use meow::settings::{self, Settings, WindowSize};

use crate::ui::{menus, photo_picker};

use super::App;

/// Where a theme from a photo is at.
enum Step {
    /// `sips` is shrinking the photo.
    Preparing(Receiver<Result<PathBuf, String>>),
    /// Claude is looking at the shrunk copy.
    Looking(Run),
}

#[derive(Default)]
pub(super) struct CatPhotoState {
    step: Option<Step>,
    /// The file name the user picked, which may hold the cat's name.
    photo_name: String,
}

impl CatPhotoState {
    pub(super) fn is_running(&self) -> bool {
        self.step.is_some()
    }
}

/// Saved settings and where to save them; `None` when the file could not be
/// read, so it is never overwritten, plus the reason.
pub(super) fn load_settings() -> (Settings, Option<PathBuf>, Option<String>) {
    let Some(path) = settings::default_path() else {
        return (Settings::default(), None, None);
    };
    match Settings::load(&path) {
        Ok(saved) => (saved, Some(path), None),
        Err(error) => (
            Settings::default(),
            None,
            Some(format!("{error}. Theme changes won't be saved.")),
        ),
    }
}

/// A folder of this launch's own for the shrunk photo, the only one the run may read.
fn photo_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("meow-cat-photo-{}", std::process::id()));
    // The run's permission rule must name the real path, not `/var`'s symlink.
    let _ = std::fs::create_dir_all(&dir);
    dir.canonicalize().unwrap_or(dir)
}

impl App {
    /// Index into `Settings::coats`, as tagged on the theme menu items.
    pub(super) fn set_coat(&mut self, index: usize) {
        let Some((_, coat)) = self.settings.coats().get(index).cloned() else {
            return;
        };
        self.apply_settings(Settings {
            coat,
            ..self.settings.clone()
        });
    }

    /// Saves the size the bubble opens at and resizes it now.
    pub(super) fn set_window_size(&mut self, index: usize) {
        let Some(window_size) = WindowSize::ALL.get(index).copied() else {
            return;
        };
        self.chat.set_size(window_size.points());
        self.apply_settings(Settings {
            window_size,
            ..self.settings.clone()
        });
    }

    pub(super) fn remove_coat(&mut self) {
        self.apply_settings(self.settings.without_current_custom());
    }

    /// Asks for a photo of the user's cat and starts making a theme from it.
    pub(super) fn add_coat_from_photo(&mut self) {
        if self.cat_photo.is_running() {
            return;
        }
        let Some(photo) = photo_picker::pick(self.mtm) else {
            return;
        };
        if !matches!(self.claude, Some(Ok(_))) {
            return self.coat_failed("Claude Code is not ready yet. Try again in a moment.");
        }
        let (sender, prepared) = mpsc::channel();
        let dir = photo_dir();
        let source = photo.clone();
        thread::spawn(move || {
            let _ = sender.send(cat_photo::prepare(&source, &dir));
        });
        self.cat_photo = CatPhotoState {
            step: Some(Step::Preparing(prepared)),
            photo_name: photo
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        self.refill_coat_menu();
    }

    /// Moves the theme from a photo along: from the shrunk copy to Claude,
    /// and from Claude's colors to a new theme.
    pub(super) fn poll_cat_photo(&mut self) {
        let outcome = match &self.cat_photo.step {
            None => return,
            Some(Step::Preparing(prepared)) => match prepared.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Ok(preview)) => return self.look_at_photo(&preview),
                Ok(Err(error)) => Err(error),
                Err(TryRecvError::Disconnected) => Err("Could not prepare the photo.".into()),
            },
            Some(Step::Looking(run)) => match outcome(run) {
                None => return,
                Some(result) => result,
            },
        };
        self.cat_photo.step = None;
        match outcome {
            Ok(custom) => self.add_custom_coat(custom),
            Err(error) => {
                self.refill_coat_menu();
                self.coat_failed(&error);
            }
        }
    }

    fn look_at_photo(&mut self, preview: &Path) {
        let Some(Ok(env)) = &self.claude else {
            self.cat_photo.step = None;
            self.refill_coat_menu();
            return self.coat_failed("Claude Code is not available.");
        };
        let prompt = cat_photo::prompt(preview, &self.cat_photo.photo_name);
        let dir = preview.parent().unwrap_or(preview);
        match Run::start_reading(env, &prompt, &env.model, dir) {
            Ok(run) => self.cat_photo.step = Some(Step::Looking(run)),
            Err(error) => {
                self.cat_photo.step = None;
                self.refill_coat_menu();
                self.coat_failed(&error);
            }
        }
    }

    fn add_custom_coat(&mut self, custom: CustomCoat) {
        let note = format!(
            "New theme: {}. Switch themes from the cat's menu › Theme.",
            custom.name
        );
        self.apply_settings(self.settings.with_custom(custom));
        self.info_in(&ChatId::Main, &note);
    }

    /// Wears and saves `settings`, and shows them in the theme menu.
    fn apply_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.cat.set_pig(self.settings.coat.is_pig());
        self.refill_coat_menu();
        self.chat.set_coat(self.settings.coat);
        // Redraw now, even if the cat holds still.
        self.last_look = None;
        let Some(path) = &self.settings_path else {
            return;
        };
        if let Err(error) = self.settings.save(path) {
            eprintln!("meow: {error}");
            self.error_in(&ChatId::Main, &error);
        }
    }

    fn refill_coat_menu(&self) {
        menus::fill_coat_menu(
            self.mtm,
            &self.coat_menu,
            &self.controller,
            &self.settings,
            self.cat_photo.is_running(),
        );
    }

    /// Shows why no theme was made, with `!` over the cat.
    fn coat_failed(&mut self, reason: &str) {
        eprintln!("meow: no theme from the photo: {reason}");
        self.error_in(
            &ChatId::Main,
            &format!("Could not make a theme from your photo: {reason}"),
        );
        if !(self.chat.is_visible() && self.conversations.active() == &ChatId::Main) {
            self.unread = true;
            self.unread_chat = ChatId::Main;
        }
    }
}

/// `None` while Claude is looking; then the theme, or why there is none.
fn outcome(run: &Run) -> Option<Result<CustomCoat, String>> {
    loop {
        return match run.events.try_recv() {
            Ok(ClaudeEvent::Done {
                is_error: false,
                result,
            }) => Some(cat_photo::parse(&result)),
            Ok(ClaudeEvent::Done {
                is_error: true,
                result,
            }) => Some(Err(result)),
            Ok(ClaudeEvent::Failed(reason)) => Some(Err(reason)),
            Ok(_) => continue,
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the run stopped early".to_string())),
        };
    }
}
