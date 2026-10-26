//! How Meow looks, from the cat's menu › Theme: the coat the cat wears and
//! the size the chat bubble opens at, kept across launches, and themes made
//! from a photo of the user's cat: the photo is shrunk in the background,
//! then a run that may only read that copy reports its colors.

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use meow::appearance::{Appearance, WindowSize};
use meow::cat_photo::{self, CustomCoat};
use meow::claude::{ClaudeEvent, Run};
use meow::conversations::ChatId;
use meow::sprites::Coat;
use objc2::MainThreadMarker;

use crate::app::feature::{Feature, Host};
use crate::app::mailbox::{Mailbox, Sender};
use crate::ui::menus::{MenuEntry, MenuSection};
use crate::ui::photo_picker;

#[derive(Clone)]
enum Msg {
    Wear(Coat),
    Resize(WindowSize),
    AddFromPhoto,
    RemoveCustom,
}

/// Where a theme from a photo is at.
enum Step {
    /// `sips` is shrinking the photo.
    Preparing(Receiver<Result<PathBuf, String>>),
    /// Claude is looking at the shrunk copy.
    Looking(Run),
}

pub(crate) struct Theme {
    appearance: Appearance,
    /// Why the saved appearance could not be read.
    load_error: Option<String>,
    /// The cat is not dressed yet: done at the first poll.
    first: bool,
    step: Option<Step>,
    /// The file name the user picked, which may hold the cat's name.
    photo_name: String,
    /// Asks the user for a photo of their cat; `None` when they cancel.
    pick_photo: fn() -> Option<PathBuf>,
    inbox: Mailbox<Msg>,
}

impl Theme {
    /// The saved appearance, or why it could not be read (then the defaults).
    pub(crate) fn from_settings(saved: Result<Appearance, String>) -> Self {
        Self::new(saved, || {
            MainThreadMarker::new().and_then(photo_picker::pick)
        })
    }

    fn new(saved: Result<Appearance, String>, pick_photo: fn() -> Option<PathBuf>) -> Self {
        let (appearance, load_error) = match saved {
            Ok(appearance) => (appearance, None),
            Err(error) => (Appearance::default(), Some(error)),
        };
        Self {
            appearance,
            load_error,
            first: true,
            step: None,
            photo_name: String::new(),
            pick_photo,
            inbox: Mailbox::new(),
        }
    }

    /// Wears and saves `appearance`, and shows it in the menu.
    fn apply(&mut self, appearance: Appearance, host: &mut dyn Host) {
        if appearance.coat != self.appearance.coat {
            host.wear(appearance.coat);
        }
        self.appearance = appearance;
        host.menu_changed();
        if let Err(error) = host.settings().set(&self.appearance) {
            eprintln!("meow: {error}");
            host.error_in(&ChatId::Main, &error);
        }
    }

    /// Asks for a photo of the user's cat and starts making a theme from it.
    fn add_from_photo(&mut self, host: &mut dyn Host) {
        if self.step.is_some() {
            return;
        }
        let Some(photo) = (self.pick_photo)() else {
            return;
        };
        if host.claude().is_none() {
            return failed(host, "Claude Code is not ready yet. Try again in a moment.");
        }
        let (sender, prepared) = mpsc::channel();
        let dir = photo_dir();
        let source = photo.clone();
        thread::spawn(move || {
            let _ = sender.send(cat_photo::prepare(&source, &dir));
        });
        self.step = Some(Step::Preparing(prepared));
        self.photo_name = photo
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        host.menu_changed();
    }

    /// Moves the theme from a photo along: from the shrunk copy to Claude,
    /// and from Claude's colors to a new theme.
    fn poll_photo(&mut self, host: &mut dyn Host) {
        let outcome = match &self.step {
            None => return,
            Some(Step::Preparing(prepared)) => match prepared.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Ok(preview)) => return self.look_at_photo(&preview, host),
                Ok(Err(error)) => Err(error),
                Err(TryRecvError::Disconnected) => Err("Could not prepare the photo.".into()),
            },
            Some(Step::Looking(run)) => match outcome(run) {
                None => return,
                Some(result) => result,
            },
        };
        self.step = None;
        match outcome {
            Ok(custom) => self.add_custom(custom, host),
            Err(error) => {
                host.menu_changed();
                failed(host, &error);
            }
        }
    }

    fn look_at_photo(&mut self, preview: &Path, host: &mut dyn Host) {
        let Some(env) = host.claude() else {
            self.step = None;
            host.menu_changed();
            return failed(host, "Claude Code is not available.");
        };
        let prompt = cat_photo::prompt(preview, &self.photo_name);
        let dir = preview.parent().unwrap_or(preview);
        match Run::start_reading(env, &prompt, &env.model, dir) {
            Ok(run) => self.step = Some(Step::Looking(run)),
            Err(error) => {
                self.step = None;
                host.menu_changed();
                failed(host, &error);
            }
        }
    }

    fn add_custom(&mut self, custom: CustomCoat, host: &mut dyn Host) {
        let note = format!(
            "New theme: {}. Switch themes from the cat's menu › Theme.",
            custom.name
        );
        self.apply(self.appearance.with_custom(custom), host);
        host.info_in(&ChatId::Main, &note);
    }

    /// The Theme submenu's items: one per theme, the worn one checked, the
    /// built-in ones apart from the user's own, then adding one from a photo
    /// and removing the worn one when it is the user's, then window sizes.
    fn theme_entries(&self) -> Vec<MenuEntry> {
        let inbox = self.inbox.sender();
        let built_in = Coat::ALL.len();
        let mut entries = Vec::new();
        for (index, (name, coat)) in self.appearance.coats().into_iter().enumerate() {
            if index == built_in {
                entries.push(MenuEntry::Separator);
            }
            let checked = coat == self.appearance.coat;
            entries.push(action(&inbox, name, checked, Msg::Wear(coat)));
        }
        entries.push(MenuEntry::Separator);
        if self.step.is_some() {
            entries.push(MenuEntry::Label("Looking at Your Cat…".to_string()));
        } else {
            let title = "Add from Cat Photo…".to_string();
            entries.push(action(&inbox, title, false, Msg::AddFromPhoto));
        }
        if let Some(custom) = self.appearance.current_custom() {
            let title = format!("Remove \u{201c}{}\u{201d}", custom.name);
            entries.push(action(&inbox, title, false, Msg::RemoveCustom));
        }
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Label("Window Size".to_string()));
        for size in WindowSize::ALL {
            let checked = size == self.appearance.window_size;
            entries.push(action(
                &inbox,
                size.name().to_string(),
                checked,
                Msg::Resize(size),
            ));
        }
        entries
    }
}

impl Feature for Theme {
    fn poll(&mut self, host: &mut dyn Host) {
        if std::mem::take(&mut self.first) {
            host.wear(self.appearance.coat);
            host.resize_chat(self.appearance.window_size.points());
        }
        for message in self.inbox.drain() {
            match message {
                Msg::Wear(coat) => self.apply(
                    Appearance {
                        coat,
                        ..self.appearance.clone()
                    },
                    host,
                ),
                // Even at the same size: it puts back a bubble resized by hand.
                Msg::Resize(window_size) => {
                    host.resize_chat(window_size.points());
                    self.apply(
                        Appearance {
                            window_size,
                            ..self.appearance.clone()
                        },
                        host,
                    );
                }
                Msg::AddFromPhoto => self.add_from_photo(host),
                Msg::RemoveCustom => self.apply(self.appearance.without_current_custom(), host),
            }
        }
        self.poll_photo(host);
    }

    fn startup_error(&self) -> Option<String> {
        self.load_error.as_deref().map(crate::app::unsaved)
    }

    fn menu(&self) -> Vec<MenuEntry> {
        vec![MenuEntry::Submenu {
            title: "Theme".to_string(),
            entries: self.theme_entries(),
        }]
    }

    fn menu_section(&self) -> MenuSection {
        MenuSection::Cat
    }

    fn is_busy(&self) -> bool {
        self.step.is_some() || !self.inbox.is_empty()
    }

    /// Looking at a photo of the user's cat takes a while.
    fn thinking(&self) -> bool {
        self.step.is_some()
    }
}

/// A menu item sending `message` each time it is chosen.
fn action(inbox: &Sender<Msg>, title: String, checked: bool, message: Msg) -> MenuEntry {
    let inbox = inbox.clone();
    MenuEntry::Action {
        title,
        checked,
        run: Box::new(move || inbox.send(message.clone())),
    }
}

/// Shows why no theme was made, with `!` over the cat.
fn failed(host: &mut dyn Host, reason: &str) {
    eprintln!("meow: no theme from the photo: {reason}");
    host.error_in(
        &ChatId::Main,
        &format!("Could not make a theme from your photo: {reason}"),
    );
    host.flag_unread(&ChatId::Main);
}

/// A folder of this launch's own for the shrunk photo, the only one the run may read.
fn photo_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("meow-cat-photo-{}", std::process::id()));
    // The run's permission rule must name the real path, not `/var`'s symlink.
    let _ = std::fs::create_dir_all(&dir);
    dir.canonicalize().unwrap_or(dir)
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
