//! Slack, and what Meow does with it: the live conversations and their page
//! (`conversations`), Claude's private panel (`assist`), drafting in the
//! user's own style (`voice`), custom emoji and attached images (`emoji`,
//! `downloads`), calendar events from a conversation (`events`), and the work
//! Autopilot prepares from Slack messages, with its own page (`autopilot`).
//!
//! They share the Slack state, so they live in one feature; each part keeps
//! its own file. Coding agents, the calendar tools and Activity are the
//! app's, reached through `Host`.

mod assist;
mod autopilot;
mod conversations;
mod downloads;
mod emoji;
mod events;
mod snapshot;
#[cfg(test)]
mod tests;
mod voice;

use std::time::{SystemTime, UNIX_EPOCH};

use meow::activity::Link;
use objc2::MainThreadMarker;
use serde::{Deserialize, Serialize};

use crate::app::feature::{Feature, Host};
use crate::app::mailbox::Mailbox;
use crate::ui::autopilot_page::{self, AutopilotAction, AutopilotPage};
use crate::ui::chat_window::{PageKit, PageView};
use crate::ui::sidebar::{Page, PageSpec};
use crate::ui::slack_page::{self, SlackAction, SlackPage};
use crate::ui::theme;

use assist::SlackAssist;
use autopilot::AutopilotState;
use conversations::SlackState;
use emoji::EmojiState;
use voice::VoiceState;

/// The Slack feature's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SlackSettings {
    /// Slack drafts follow the user's learned style guide.
    pub write_as_me: bool,
    /// Slack messages asking the user for something start autopilot work.
    pub autopilot: bool,
}

enum Msg {
    Slack(SlackAction),
    Autopilot(AutopilotAction),
}

pub(crate) struct Slack {
    /// The live Slack connection and conversation state.
    slack: SlackState,
    /// The private Claude question about the open conversation.
    slack_assist: SlackAssist,
    /// The workspace's custom emoji, and their images once downloaded.
    emoji: EmojiState,
    /// The user's learned writing style, for drafts.
    voice: VoiceState,
    /// Work started from Slack messages, waiting for approval.
    autopilot: AutopilotState,
    settings: SlackSettings,
    /// Why saved data could not be read, told at launch.
    load_errors: Vec<String>,
    page: Option<SlackPage>,
    autopilot_page: Option<AutopilotPage>,
    /// The Autopilot page and its count need drawing again.
    autopilot_dirty: bool,
    /// The coding agents' revision last seen, to redraw the panel showing one.
    agents_seen: u64,
    inbox: Mailbox<Msg>,
}

impl Slack {
    /// Connects with the saved session, if any. `saved` are its settings, or
    /// why they could not be read.
    pub(crate) fn from_settings(saved: Result<SlackSettings, String>) -> Self {
        let (autopilot, autopilot_error) = AutopilotState::load();
        Self::new(
            saved,
            SlackState::new(),
            VoiceState::load(),
            autopilot,
            autopilot_error,
        )
    }

    /// Signed out, with no saved data: nothing connects or is read, for tests.
    #[cfg(test)]
    fn signed_out(saved: Result<SlackSettings, String>) -> Self {
        Self::new(
            saved,
            SlackState::default(),
            VoiceState::empty(),
            AutopilotState::empty(),
            None,
        )
    }

    fn new(
        saved: Result<SlackSettings, String>,
        slack: SlackState,
        voice: VoiceState,
        autopilot: AutopilotState,
        autopilot_error: Option<String>,
    ) -> Self {
        let (settings, settings_error) = match saved {
            Ok(settings) => (settings, None),
            Err(error) => (SlackSettings::default(), Some(crate::app::unsaved(&error))),
        };
        Self {
            slack,
            slack_assist: SlackAssist::default(),
            emoji: EmojiState::default(),
            voice,
            autopilot,
            settings,
            load_errors: [settings_error, autopilot_error]
                .into_iter()
                .flatten()
                .collect(),
            page: None,
            autopilot_page: None,
            autopilot_dirty: true,
            agents_seen: 0,
            inbox: Mailbox::new(),
        }
    }

    /// The Slack page, built at launch before any poll.
    fn page(&self) -> &SlackPage {
        self.page
            .as_ref()
            .expect("the Slack page is built at launch")
    }

    /// The Autopilot page, built at launch before any poll.
    fn autopilot_page(&self) -> &AutopilotPage {
        self.autopilot_page
            .as_ref()
            .expect("the Autopilot page is built at launch")
    }

    fn slack_visible(&self, host: &dyn Host) -> bool {
        host.page_visible(slack_page::PAGE_ID)
    }
}

impl Feature for Slack {
    fn poll(&mut self, host: &mut dyn Host) {
        for message in self.inbox.drain() {
            match message {
                Msg::Slack(action) => self.slack_action(action, host),
                Msg::Autopilot(action) => self.autopilot_action(action, host),
            }
        }
        self.poll_slack(host);
        self.poll_emoji(host);
        self.animate_slack(host);
        self.poll_slack_assist(host);
        self.poll_voice(host);
        self.poll_autopilot(host);
        if host.agents_revision() != self.agents_seen {
            self.agents_seen = host.agents_revision();
            self.redraw_shown_agent(host);
        }
        if std::mem::take(&mut self.autopilot_dirty) {
            self.render_autopilot(host);
        }
    }

    fn startup_error(&self) -> Option<String> {
        (!self.load_errors.is_empty()).then(|| self.load_errors.join("\n"))
    }

    fn pages(&self) -> Vec<PageSpec> {
        vec![
            PageSpec {
                id: slack_page::PAGE_ID,
                title: "Slack",
                badge: theme::accent,
                order: 30,
            },
            PageSpec {
                id: autopilot_page::PAGE_ID,
                title: "Autopilot",
                badge: theme::accent,
                order: 50,
            },
        ]
    }

    fn build_page(&mut self, id: &'static str, kit: &PageKit) -> Option<PageView> {
        let inbox = self.inbox.sender();
        match id {
            slack_page::PAGE_ID => {
                let page = SlackPage::new(kit.mtm, kit.controller, kit.frame, move |action| {
                    inbox.send(Msg::Slack(action))
                });
                let view = PageView {
                    view: page.view.clone(),
                    focus: Some(page.input_view()),
                };
                self.page = Some(page);
                Some(view)
            }
            autopilot_page::PAGE_ID => {
                let page = AutopilotPage::new(kit.mtm, kit.frame, move |action| {
                    inbox.send(Msg::Autopilot(action))
                });
                let view = PageView {
                    view: page.view.clone(),
                    focus: None,
                };
                self.autopilot_page = Some(page);
                self.autopilot_dirty = true;
                Some(view)
            }
            _ => None,
        }
    }

    fn page_shown(&mut self, id: &'static str) {
        match id {
            slack_page::PAGE_ID => self.render_slack(),
            _ => self.autopilot_dirty = true,
        }
    }

    /// A Slack message opens its conversation, scrolled to it; Autopilot
    /// news opens the Autopilot page.
    fn open_link(&mut self, link: &Link, host: &mut dyn Host) -> bool {
        match link {
            Link::Slack {
                channel,
                ts,
                thread_ts,
            } => {
                host.open_page(Page::Feature(slack_page::PAGE_ID));
                match ts {
                    Some(ts) => self.open_slack_message(
                        channel.clone(),
                        ts.clone(),
                        thread_ts.clone(),
                        host,
                    ),
                    None => self.open_slack_channel(channel.clone(), host),
                }
                true
            }
            Link::Autopilot => {
                // Its page is drawn again, as `page_shown` would.
                self.autopilot_dirty = true;
                host.open_page(Page::Feature(autopilot_page::PAGE_ID));
                true
            }
            _ => false,
        }
    }

    /// Esc closes Claude's panel, then goes back from a thread or a
    /// conversation; on the list, the bubble closes.
    fn escape(&mut self, host: &mut dyn Host) -> bool {
        if !self.slack_visible(host) {
            return false;
        }
        if self.slack_assist_shown() {
            self.close_slack_assist();
            return true;
        }
        if self.page().in_conversation() {
            self.slack_back(host);
            return true;
        }
        false
    }

    fn quit(&mut self) {
        self.slack.save_cache();
    }

    fn snapshot_steps(&self) -> Vec<&'static str> {
        snapshot::STEPS.to_vec()
    }

    fn show_snapshot(&mut self, step: &'static str, host: &mut dyn Host) {
        self.show_sample(step, host);
    }

    /// A question to Claude about a conversation is under way.
    fn is_busy(&self) -> bool {
        self.slack_assist.run.is_some()
    }
}

/// The main-thread marker, as the feature only runs on the main thread.
fn main_thread() -> Option<MainThreadMarker> {
    MainThreadMarker::new()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
