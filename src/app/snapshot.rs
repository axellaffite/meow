//! Debug snapshots of the bubble, for checking the look without screen access.
//!
//! With `MEOW_SNAPSHOT_DIR` set, Meow opens the bubble a few seconds after
//! launch, shows each page in turn, then the steps the features add (each
//! saved under its name), saves it as a PNG in that folder, then quits.
//! Features show sample data where they have the user's (the Slack feature
//! never signs in; launch with dummy `MEOW_SLACK_*` values to keep off the
//! Keychain).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use meow::transcript::Transcript;

use crate::ui::events::UiEvent;

use super::App;

/// The app's pages, in order; each is saved once it has settled.
const STEPS: [&str; 6] = [
    "chat",
    "chat-two-lines",
    "chat-long",
    "tasks",
    "tasks-reply",
    "plan",
];
/// Time for the app to settle before the first step (fonts, tasks).
const START_DELAY: Duration = Duration::from_secs(4);
const STEP_DELAY: Duration = Duration::from_millis(2500);

pub(super) struct Snapshot {
    dir: PathBuf,
    started: Instant,
    /// Index of the next step to start; a step's file is saved before the next one.
    next: usize,
}

impl Snapshot {
    pub(super) fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("MEOW_SNAPSHOT_DIR")?);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("meow: snapshot folder {}: {error}", dir.display());
            return None;
        }
        Some(Self {
            dir,
            started: Instant::now(),
            next: 0,
        })
    }
}

impl App {
    /// The app's pages, then the features' steps.
    fn snapshot_steps(&self) -> Vec<&'static str> {
        STEPS
            .into_iter()
            .chain(self.feature_snapshot_steps())
            .collect()
    }

    pub(super) fn poll_snapshot(&mut self) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let steps = self.snapshot_steps();
        let next = snapshot.next;
        if snapshot.started.elapsed() < START_DELAY + STEP_DELAY * next as u32 {
            return;
        }
        if next == 0 && !self.chat.is_visible() {
            self.handle(UiEvent::ToggleChat);
        }
        if next > 0 {
            self.save_snapshot(steps[next - 1]);
        }
        if next == steps.len() {
            eprintln!("meow: snapshots saved");
            std::process::exit(0);
        }
        self.show_snapshot_step(steps[next]);
        if let Some(snapshot) = self.snapshot.as_mut() {
            snapshot.next += 1;
        }
    }

    fn show_snapshot_step(&mut self, step: &'static str) {
        match step {
            "chat" => {
                self.handle(UiEvent::ShowChat);
                self.chat.chat().render(&sample_chat(), true);
            }
            "chat-two-lines" => self.chat.chat().set_draft(
                "Rédige une relance pour la relecture Android, en restant court et sympa avec Hugo.",
            ),
            "chat-long" => self.chat.chat().set_draft(
                "Rédige une relance pour la relecture Android.\nReste court et sympa.\n\
                 Mentionne que la mise en prod attend cette PR.\nEt propose un créneau demain.",
            ),
            "tasks" => {
                self.chat.chat().set_draft("");
                self.handle(UiEvent::ShowTasks);
            }
            "tasks-reply" => {
                // The first task asks whether to draft a reply; never ticked for real.
                if let Some(id) = self.chat.tasks().id_at(0) {
                    self.pending_done = Some((id, Instant::now() + Duration::from_secs(3600)));
                    self.tasks_changed();
                }
            }
            "plan" => self.handle(UiEvent::QuickAction(0)),
            // A step a feature adds.
            step => self.show_feature_snapshot(step),
        }
    }

    fn save_snapshot(&self, name: &str) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let path = snapshot.dir.join(format!("{name}.png"));
        match self.chat.snapshot(&path) {
            Ok(()) => eprintln!("meow: saved {}", path.display()),
            Err(error) => eprintln!("meow: snapshot {name}: {error}"),
        }
    }
}

fn sample_chat() -> Transcript {
    let mut chat = Transcript::default();
    chat.info("Click the cat or press ⌥⇧Space to talk. Right-click for options.");
    chat.start_turn("Tu peux résumer le fil #release de ce matin ?");
    chat.tool("slack_read_channel #release");
    chat.tool("slack_read_thread");
    chat.text(
        "Voici l'essentiel :\n\n- build iOS **validée**\n- la PR Android attend une relecture\n\
         - mise en prod après le standup",
    );
    chat.finish_turn("");
    chat.start_turn("Rédige une relance pour la relecture Android.");
    chat.text("Proposition :\n\n> Petite relance sur la PR Android : elle bloque la mise en prod.");
    chat.finish_turn("");
    chat
}
