//! Keeps a release build up to date: checks `latest` now and then in the
//! background, stages a newer build beside the app, then swaps it in and
//! relaunches once nothing is running and the chat is closed.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use meow::conversations::ChatId;
use meow::update::{self, Manifest};

use super::App;
use crate::ui::events::UiEvent;
use crate::ui::notify;

/// The first check waits for the app to settle.
const FIRST_DELAY: Duration = Duration::from_secs(60);
/// After a failed check (offline, repository private…).
const RETRY_DELAY: Duration = Duration::from_secs(30 * 60);

/// Where to answer a check the user asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Asked {
    /// From the cat's menu: a macOS banner.
    Banner,
    /// With `/update` in this conversation.
    Chat(ChatId),
}

/// A newer build, unpacked beside the app.
struct Staged {
    manifest: Manifest,
    path: PathBuf,
}

pub(super) struct UpdateState {
    /// The installed app and this build's commit; `None` for local builds or
    /// with `MEOW_UPDATE_HOURS=0`, which never update.
    target: Option<(PathBuf, &'static str)>,
    interval: Duration,
    next_at: Instant,
    check: Option<Receiver<Result<Option<Staged>, String>>>,
    staged: Option<Staged>,
    /// Why `target` is `None`, to tell the user who asks for a check.
    why_not: Option<&'static str>,
    /// The user asked for the check under way (or about to start).
    asked: Option<Asked>,
}

impl UpdateState {
    pub(super) fn from_env() -> Self {
        let interval = update::check_interval(std::env::var("MEOW_UPDATE_HOURS").ok().as_deref());
        let snapshot = std::env::var_os("MEOW_SNAPSHOT_DIR").is_some();
        let bundle = std::env::current_exe()
            .ok()
            .and_then(|exe| update::bundle_of(&exe));
        let why_not = update::why_not_updating(update::own_commit(), bundle.is_some(), interval);
        let target = match (bundle, update::own_commit(), interval) {
            (Some(bundle), Some(own), Some(_)) if !snapshot => Some((bundle, own)),
            _ => None,
        };
        Self {
            target,
            interval: interval.unwrap_or(RETRY_DELAY),
            next_at: Instant::now() + FIRST_DELAY,
            check: None,
            staged: None,
            why_not,
            asked: None,
        }
    }
}

/// "Version 90220bc", for the cat's menu.
pub(super) fn version_label() -> String {
    match update::own_commit() {
        Some(commit) => format!("Version {}", update::short(commit)),
        None => "Version: local build".to_string(),
    }
}

impl App {
    /// Checks for a new version now, answering where the user asked.
    pub(super) fn check_for_updates(&mut self, asked: Asked) {
        if self.update.target.is_none() {
            let why = self
                .update
                .why_not
                .unwrap_or("Updates are not available in this mode.");
            return self.tell_update(&asked, why, false);
        }
        if let Some(staged) = &self.update.staged {
            let message = update::outcome("", &Ok(Some(staged.manifest.short().to_string())));
            return self.tell_update(&asked, &message, false);
        }
        if self.update.check.is_none() {
            self.update.next_at = Instant::now();
        }
        if let Asked::Chat(id) = &asked {
            self.info_in(id, "Checking for updates…");
        }
        self.update.asked = Some(asked);
    }

    fn tell_update(&mut self, asked: &Asked, message: &str, failed: bool) {
        match asked {
            Asked::Banner => notify::post("Meow", message),
            Asked::Chat(id) if failed => self.error_in(id, message),
            Asked::Chat(id) => self.info_in(id, message),
        }
    }

    pub(super) fn poll_update(&mut self) {
        let Some((bundle, own)) = self.update.target.clone() else {
            return;
        };
        if self.update.staged.is_some() {
            if self.idle_for_update() {
                self.apply_update(&bundle);
            }
            return;
        }
        if let Some(check) = &self.update.check {
            let result = match check.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("update check stopped".to_string()),
            };
            self.update.check = None;
            self.update.next_at = Instant::now()
                + match &result {
                    Ok(_) => self.update.interval,
                    Err(_) => RETRY_DELAY.min(self.update.interval),
                };
            if let Some(asked) = self.update.asked.take() {
                let found = result
                    .as_ref()
                    .map(|staged| staged.as_ref().map(|s| s.manifest.short().to_string()))
                    .map_err(Clone::clone);
                let message = update::outcome(own, &found);
                self.tell_update(&asked, &message, found.is_err());
            }
            match result {
                Ok(Some(staged)) => {
                    eprintln!("meow: update {} ready", staged.manifest.short());
                    self.update.staged = Some(staged);
                }
                Ok(None) => {}
                Err(error) => eprintln!("meow: {error}"),
            }
            return;
        }
        if Instant::now() < self.update.next_at {
            return;
        }
        let Some(work) = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Caches/Meow/update"))
        else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = update::fetch_latest().and_then(|latest| {
                if !update::is_update(own, &latest) {
                    return Ok(None);
                }
                let path = update::stage(&latest, &bundle, &work)?;
                Ok(Some(Staged {
                    manifest: latest,
                    path,
                }))
            });
            let _ = tx.send(result);
        });
        self.update.check = Some(rx);
    }

    /// Nothing the user would lose to a restart: no run, no open chat.
    fn idle_for_update(&self) -> bool {
        self.runs.is_empty()
            && !self.chat.is_visible()
            && !self.sync.is_running()
            && !self.agenda.is_running()
            && !self.rsvp.is_running()
            && self.routines.running().is_none()
            && self.slack_assist.run.is_none()
            && !self.agents.is_running()
    }

    fn apply_update(&mut self, bundle: &std::path::Path) {
        let Some(staged) = self.update.staged.take() else {
            return;
        };
        let swapped = update::swap(&staged.path, bundle).and_then(|()| update::relaunch(bundle));
        match swapped {
            Ok(()) => {
                eprintln!("meow: updated to {}, relaunching", staged.manifest.short());
                self.handle(UiEvent::Quit);
            }
            Err(error) => {
                eprintln!("meow: {error}");
                let _ = std::fs::remove_dir_all(&staged.path);
                self.update.next_at = Instant::now() + RETRY_DELAY;
            }
        }
    }
}
