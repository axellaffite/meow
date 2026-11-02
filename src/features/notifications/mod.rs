//! Which news interrupts: Settings › Notifications turns macOS banners off
//! altogether, or for some kinds of news (Slack messages, calendar changes,
//! new tasks, scheduled tasks, Autopilot). Everything still goes to the
//! Activity page; only the banner goes.

#[cfg(test)]
mod tests;

use meow::activity::{Banners, Kind};
use serde::{Deserialize, Serialize};

use crate::app::feature::{Feature, Host};
use crate::app::mailbox::Mailbox;
use crate::ui::settings_page::{SettingEntry, SettingsSection};

/// Saved with the other settings, under `banners`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct NotificationSettings {
    banners: Banners,
}

#[derive(Clone, Copy)]
enum Msg {
    /// Banners on or off altogether.
    Banners(bool),
    /// Banners for one kind of news on or off.
    Kind(Kind, bool),
}

pub(crate) struct Notifications {
    settings: NotificationSettings,
    /// Why the saved settings could not be read.
    load_error: Option<String>,
    /// The app has not been told the settings yet: done at the first poll.
    first: bool,
    inbox: Mailbox<Msg>,
}

impl Notifications {
    /// The saved settings, or the defaults (every banner) when they cannot be read.
    pub(crate) fn from_settings(saved: Result<NotificationSettings, String>) -> Self {
        let (settings, load_error) = match saved {
            Ok(settings) => (settings, None),
            Err(error) => (NotificationSettings::default(), Some(error)),
        };
        Self {
            settings,
            load_error,
            first: true,
            inbox: Mailbox::new(),
        }
    }

    fn handle(&mut self, message: Msg, host: &mut dyn Host) {
        let banners = &mut self.settings.banners;
        match message {
            Msg::Banners(on) => banners.on = on,
            Msg::Kind(kind, on) => {
                banners.muted.retain(|muted| *muted != kind);
                if !on {
                    banners.muted.push(kind);
                }
            }
        }
        host.set_banners(self.settings.banners.clone());
        host.settings_changed();
        if let Err(error) = host.settings().set(&self.settings) {
            eprintln!("meow: {error}");
        }
    }

    fn toggle(&self, title: &str, on: bool, message: Msg) -> SettingEntry {
        let inbox = self.inbox.sender();
        SettingEntry::Toggle {
            title: title.to_string(),
            on,
            run: Box::new(move || inbox.send(message)),
        }
    }
}

/// How Settings names each kind of news.
fn kind_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Slack => "Slack messages",
        Kind::Calendar => "Calendar changes",
        Kind::Tasks => "New tasks",
        Kind::Scheduled => "Scheduled tasks",
        Kind::Autopilot => "Autopilot",
    }
}

impl Feature for Notifications {
    fn poll(&mut self, host: &mut dyn Host) {
        if std::mem::take(&mut self.first) {
            host.set_banners(self.settings.banners.clone());
        }
        for message in self.inbox.drain() {
            self.handle(message, host);
        }
    }

    fn startup_error(&self) -> Option<String> {
        self.load_error.clone()
    }

    fn settings(&self) -> Vec<SettingsSection> {
        let banners = &self.settings.banners;
        let mut entries =
            vec![self.toggle("Show macOS banners", banners.on, Msg::Banners(!banners.on))];
        if banners.on {
            for kind in Kind::ALL {
                let on = banners.allows(kind);
                entries.push(self.toggle(kind_title(kind), on, Msg::Kind(kind, !on)));
            }
        }
        entries.push(SettingEntry::Note(
            "News without a banner still goes to Activity. Tasks resolved at the source never \
             show one."
                .to_string(),
        ));
        vec![SettingsSection {
            title: "Notifications",
            entries,
        }]
    }
}
