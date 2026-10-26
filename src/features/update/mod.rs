//! Keeps a release build up to date: checks `latest` now and then in the
//! background, stages a newer build beside the app, then swaps it in and
//! relaunches once nothing is running and the chat is closed. The user can
//! ask for a check from the cat's menu or with `/update` in a conversation.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use meow::conversations::ChatId;
use meow::update::{self, Manifest};

use crate::app::feature::{Feature, Host};
use crate::app::mailbox::Mailbox;
use crate::ui::menus::MenuEntry;

#[cfg(test)]
mod tests;

const COMMAND: &str = "/update";
/// The first check waits for the app to settle.
const FIRST_DELAY: Duration = Duration::from_secs(60);
/// After a failed check (offline, repository private…).
const RETRY_DELAY: Duration = Duration::from_secs(30 * 60);

/// Where to answer a check the user asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    /// From the cat's menu: a macOS banner.
    Banner,
    /// With `/update` in this conversation.
    Chat(ChatId),
}

enum Msg {
    /// Check for a new version now, answering where the user asked.
    Check(Asked),
}

/// A newer build, unpacked beside the app.
struct Staged {
    manifest: Manifest,
    path: PathBuf,
}

pub(crate) struct Update {
    /// The installed app and this build's commit; `None` for local builds or
    /// with `MEOW_UPDATE_HOURS=0`, which never update.
    target: Option<(PathBuf, &'static str)>,
    /// This build's commit, for the menu; `None` for local builds.
    commit: Option<&'static str>,
    interval: Duration,
    next_at: Instant,
    check: Option<Receiver<Result<Option<Staged>, String>>>,
    staged: Option<Staged>,
    /// Why `target` is `None`, to tell the user who asks for a check.
    why_not: Option<&'static str>,
    /// The user asked for the check under way (or about to start).
    asked: Option<Asked>,
    inbox: Mailbox<Msg>,
}

impl Update {
    pub(crate) fn from_env() -> Self {
        let interval = update::check_interval(std::env::var("MEOW_UPDATE_HOURS").ok().as_deref());
        let snapshot = std::env::var_os("MEOW_SNAPSHOT_DIR").is_some();
        let bundle = std::env::current_exe()
            .ok()
            .and_then(|exe| update::bundle_of(&exe));
        let commit = update::own_commit();
        let why_not = update::why_not_updating(commit, bundle.is_some(), interval);
        let target = match (bundle, commit, interval) {
            (Some(bundle), Some(own), Some(_)) if !snapshot => Some((bundle, own)),
            _ => None,
        };
        Self::new(target, commit, interval.unwrap_or(RETRY_DELAY), why_not)
    }

    fn new(
        target: Option<(PathBuf, &'static str)>,
        commit: Option<&'static str>,
        interval: Duration,
        why_not: Option<&'static str>,
    ) -> Self {
        Self {
            target,
            commit,
            interval,
            next_at: Instant::now() + FIRST_DELAY,
            check: None,
            staged: None,
            why_not,
            asked: None,
            inbox: Mailbox::new(),
        }
    }

    fn check_now(&mut self, asked: Asked, host: &mut dyn Host) {
        if self.target.is_none() {
            let why = self
                .why_not
                .unwrap_or("Updates are not available in this mode.");
            return tell(host, &asked, why, false);
        }
        if let Some(staged) = &self.staged {
            let message = update::outcome("", &Ok(Some(staged.manifest.short().to_string())));
            return tell(host, &asked, &message, false);
        }
        if self.check.is_none() {
            self.next_at = Instant::now();
        }
        if let Asked::Chat(id) = &asked {
            host.info_in(id, "Checking for updates…");
        }
        self.asked = Some(asked);
    }

    fn poll_check(&mut self, host: &mut dyn Host) {
        let Some((bundle, own)) = self.target.clone() else {
            return;
        };
        if self.staged.is_some() {
            if host.is_idle() {
                self.apply(&bundle, host);
            }
            return;
        }
        if let Some(check) = &self.check {
            let result = match check.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("update check stopped".to_string()),
            };
            return self.checked(result, own, host);
        }
        if Instant::now() < self.next_at {
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
        self.check = Some(rx);
    }

    /// The background check finished.
    fn checked(&mut self, result: Result<Option<Staged>, String>, own: &str, host: &mut dyn Host) {
        self.check = None;
        self.next_at = Instant::now()
            + match &result {
                Ok(_) => self.interval,
                Err(_) => RETRY_DELAY.min(self.interval),
            };
        if let Some(asked) = self.asked.take() {
            let found = result
                .as_ref()
                .map(|staged| staged.as_ref().map(|s| s.manifest.short().to_string()))
                .map_err(Clone::clone);
            let message = update::outcome(own, &found);
            tell(host, &asked, &message, found.is_err());
        }
        match result {
            Ok(Some(staged)) => {
                eprintln!("meow: update {} ready", staged.manifest.short());
                self.staged = Some(staged);
            }
            Ok(None) => {}
            Err(error) => eprintln!("meow: {error}"),
        }
    }

    fn apply(&mut self, bundle: &Path, host: &mut dyn Host) {
        let Some(staged) = self.staged.take() else {
            return;
        };
        let swapped = update::swap(&staged.path, bundle).and_then(|()| update::relaunch(bundle));
        match swapped {
            Ok(()) => {
                eprintln!("meow: updated to {}, relaunching", staged.manifest.short());
                host.quit();
            }
            Err(error) => {
                eprintln!("meow: {error}");
                let _ = std::fs::remove_dir_all(&staged.path);
                self.next_at = Instant::now() + RETRY_DELAY;
            }
        }
    }
}

impl Feature for Update {
    fn poll(&mut self, host: &mut dyn Host) {
        for message in self.inbox.drain() {
            match message {
                Msg::Check(asked) => self.check_now(asked, host),
            }
        }
        self.poll_check(host);
    }

    fn command(&mut self, text: &str, chat: &ChatId) -> bool {
        let taken = text == COMMAND;
        if taken {
            self.inbox
                .sender()
                .send(Msg::Check(Asked::Chat(chat.clone())));
        }
        taken
    }

    fn menu(&self) -> Vec<MenuEntry> {
        let inbox = self.inbox.sender();
        vec![
            MenuEntry::Label(version_label(self.commit)),
            MenuEntry::Action {
                title: "Check for Updates".to_string(),
                checked: false,
                run: Box::new(move || inbox.send(Msg::Check(Asked::Banner))),
            },
        ]
    }
}

fn tell(host: &mut dyn Host, asked: &Asked, message: &str, failed: bool) {
    match asked {
        Asked::Banner => host.notify(message),
        Asked::Chat(id) if failed => host.error_in(id, message),
        Asked::Chat(id) => host.info_in(id, message),
    }
}

/// "Version 90220bc", for the cat's menu.
fn version_label(commit: Option<&str>) -> String {
    match commit {
        Some(commit) => format!("Version {}", update::short(commit)),
        None => "Version: local build".to_string(),
    }
}
