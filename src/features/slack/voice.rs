//! "Write like me": learns how the user writes from their recent Slack
//! messages, keeps that style guide in a local file, and, while turned on,
//! hands it to Claude for every Slack draft.
//!
//! Learning runs in two steps: the Slack worker fetches the messages, then a
//! tool-less Opus run turns them into the guide.

use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

use meow::claude::{ClaudeEvent, Run, STRONG_MODEL};
use meow::slack::client::Command;
use meow::slack::voice;

use crate::ui::slack_voice_bar::VoiceBarState;

use super::{Slack, SlackSettings};
use crate::app::feature::Host;

/// Where learning is.
enum Learning {
    Idle,
    /// Waiting for the user's messages from Slack.
    Fetching,
    /// Opus is writing the guide from this many messages.
    Writing {
        run: Run,
        count: usize,
    },
}

pub(super) struct VoiceState {
    path: Option<PathBuf>,
    guide: Option<String>,
    learning: Learning,
    status: String,
}

impl VoiceState {
    /// Loads the guide learned in an earlier session, if any.
    pub(super) fn load() -> Self {
        let path = voice::default_path();
        let guide = path.as_deref().and_then(voice::load);
        let status = match (&path, &guide) {
            (None, _) => "Cannot find the home folder to keep a style guide.".to_string(),
            (Some(path), Some(_)) => format!("Style guide: {}", path.display()),
            (Some(_), None) => format!(
                "Learn your style from your last {} messages to draft like you.",
                voice::SAMPLE_SIZE
            ),
        };
        Self {
            path,
            guide,
            learning: Learning::Idle,
            status,
        }
    }

    /// No guide and no file, for tests.
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            path: None,
            guide: None,
            learning: Learning::Idle,
            status: String::new(),
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.learning, Learning::Idle)
    }
}

impl Slack {
    /// The guide Claude should follow for drafts, when "write as me" is on.
    pub(super) fn voice_guide_for_drafts(&self) -> Option<&str> {
        if self.settings.write_as_me {
            self.voice.guide.as_deref()
        } else {
            None
        }
    }

    pub(super) fn voice_bar_state(&self) -> VoiceBarState<'_> {
        VoiceBarState {
            enabled: self.settings.write_as_me && self.voice.guide.is_some(),
            learned: self.voice.guide.is_some(),
            busy: self.voice.busy(),
            status: &self.voice.status,
        }
    }

    /// Turns drafting in the user's style on or off; turning it on before any
    /// guide exists learns one first.
    pub(super) fn toggle_write_as_me(&mut self, host: &mut dyn Host) {
        let on = !(self.settings.write_as_me && self.voice.guide.is_some());
        self.save_write_as_me(on, host);
        if on && self.voice.guide.is_none() && !self.voice.busy() {
            self.learn_my_style(host);
        }
        self.refresh_voice_bar(host);
    }

    /// Fetches the user's recent messages to (re)learn their style.
    pub(super) fn learn_my_style(&mut self, host: &mut dyn Host) {
        if self.voice.busy() {
            return;
        }
        if self.voice.path.is_none() {
            return;
        }
        if !self.slack.request(Command::FetchMyMessages {
            limit: voice::SAMPLE_SIZE,
        }) {
            self.voice.status = "Sign in to Slack first.".to_string();
            self.refresh_voice_bar(host);
            return;
        }
        self.voice.learning = Learning::Fetching;
        self.voice.status = format!("Reading your last {} messages…", voice::SAMPLE_SIZE);
        self.refresh_voice_bar(host);
    }

    /// The worker's answer to [`Command::FetchMyMessages`].
    pub(super) fn on_my_messages(
        &mut self,
        result: Result<Vec<String>, String>,
        host: &mut dyn Host,
    ) {
        if !matches!(self.voice.learning, Learning::Fetching) {
            return;
        }
        let messages = match result {
            Ok(messages) if messages.is_empty() => {
                return self.finish_learning(
                    Err("Found none of your messages to learn from.".into()),
                    host,
                );
            }
            Ok(messages) => messages,
            Err(error) => {
                return self
                    .finish_learning(Err(format!("Could not read your messages: {error}")), host);
            }
        };
        let prompt = voice::learn_prompt(&self.slack.my_name(), &messages);
        let started = super::assist::claude_env(host)
            .and_then(|env| Run::start_offline_on(env, &prompt, STRONG_MODEL));
        match started {
            Ok(run) => {
                let count = messages.len();
                self.voice.learning = Learning::Writing { run, count };
                self.voice.status = format!("Learning your style from {count} messages…");
                self.refresh_voice_bar(host);
            }
            Err(error) => self.finish_learning(Err(error), host),
        }
    }

    /// Collects the guide once Opus has written it.
    pub(super) fn poll_voice(&mut self, host: &mut dyn Host) {
        loop {
            let Learning::Writing { run, count } = &self.voice.learning else {
                return;
            };
            let count = *count;
            let outcome = match run.events.try_recv() {
                Ok(ClaudeEvent::Done {
                    is_error: false,
                    result,
                }) => Ok((result, count)),
                Ok(ClaudeEvent::Done {
                    is_error: true,
                    result,
                }) => Err(result),
                Ok(ClaudeEvent::Failed(reason)) => Err(reason),
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("Claude stopped early.".to_string()),
            };
            return self.finish_learning(outcome, host);
        }
    }

    fn finish_learning(&mut self, outcome: Result<(String, usize), String>, host: &mut dyn Host) {
        self.voice.learning = Learning::Idle;
        let saved = outcome.and_then(|(guide, count)| {
            let path = self
                .voice
                .path
                .clone()
                .ok_or("No place to keep the style guide.")?;
            voice::save(&path, &guide)?;
            Ok((guide, count, path))
        });
        match saved {
            Ok((guide, count, path)) => {
                self.voice.guide = Some(guide.trim().to_string());
                self.voice.status = format!("Learned from {count} messages: {}", path.display());
                self.save_write_as_me(true, host);
            }
            Err(error) => self.voice.status = error,
        }
        self.refresh_voice_bar(host);
    }

    fn save_write_as_me(&mut self, on: bool, host: &mut dyn Host) {
        self.settings = SlackSettings {
            write_as_me: on,
            ..self.settings.clone()
        };
        if let Err(error) = host.settings().set(&self.settings) {
            eprintln!("meow: {error}");
        }
    }

    fn refresh_voice_bar(&self, host: &mut dyn Host) {
        if self.slack_visible(host) {
            self.page().render_voice(&self.voice_bar_state());
        }
    }
}
