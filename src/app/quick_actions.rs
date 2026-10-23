//! Quick actions: clicking one shows its last result; [ RUN ] runs it again.

use meow::action_results::{self, ActionResults};
use meow::actions::QuickAction;
use meow::conversations::{ChatId, Conversation};
use meow::plan::DayPlan;
use meow::tasks;

use crate::ui::sidebar::Page;

use super::requests::{RunKind, STILL_WORKING};
use super::{App, now_secs};

/// The user line shown for a run of `action`.
fn run_label(action: &QuickAction) -> String {
    format!("⚡︎ {}", action.title)
}

impl App {
    /// Shows the action's last result. An action that never ran runs now; from
    /// the cat's menu with the bubble closed, it runs in the background and the
    /// cat shows `!` when done.
    pub(super) fn quick_action(&mut self, index: usize) {
        let Some(action) = self.actions.get(index) else {
            return;
        };
        let has_result = self.action_results.get(&action.title).is_some();
        if !has_result && !self.chat.is_visible() {
            self.run_action(index);
            return;
        }
        if !self.chat.is_visible() {
            self.open_window();
        }
        let id = ChatId::Action(index);
        self.show_conversation(id.clone());
        let running = self.is_running(&id);
        if !has_result && !running {
            self.run_action(index);
        }
    }

    /// The [ RUN ] button: runs the action on screen again.
    pub(super) fn rerun_shown_action(&mut self) {
        if let ChatId::Action(index) = self.conversations.active().clone() {
            self.run_action(index);
        }
    }

    /// Runs the action afresh in its conversation, replacing what was shown.
    pub(super) fn run_action(&mut self, index: usize) {
        let Some(action) = self.actions.get(index).cloned() else {
            return;
        };
        let id = ChatId::Action(index);
        if self.is_running(&id) {
            self.info_in(&id, STILL_WORKING);
            return;
        }
        self.detach_chat_agents(&id);
        self.conversations.get_mut(&id).reset();
        self.start_request(
            &id,
            &run_label(&action),
            &action.full_prompt(now_secs(), &self.tasks.recently_done(tasks::MAX_HANDLED)),
            RunKind::Action,
        );
        if self.is_running(&id)
            && self.chat.page() == Page::Plan
            && self.conversations.active() == &id
        {
            // Follow the run's progress; the new plan shows when it is done.
            self.open_conversation(id.clone(), false);
        }
        if !self.is_running(&id) && !self.chat.is_visible() {
            // It could not start; the reason is in the action's conversation.
            self.unread = true;
            self.unread_chat = id;
        }
    }

    /// The action's conversation as last saved: its result, ready for follow-ups.
    pub(super) fn saved_action_conversation(&self, index: usize) -> Conversation {
        let mut conversation = Conversation::default();
        let Some(action) = self.actions.get(index) else {
            return conversation;
        };
        if let Some(result) = self.action_results.get(&action.title) {
            conversation.session = result.session.clone();
            conversation
                .transcript
                .answered(&run_label(action), &result.answer);
        }
        conversation
    }

    /// Forgets follow-ups, keeping the result on screen.
    pub(super) fn restart_action_conversation(&mut self, index: usize) {
        let mut fresh = self.saved_action_conversation(index);
        fresh.session = None;
        *self.conversations.get_mut(&ChatId::Action(index)) = fresh;
    }

    /// "Today's planning · ran 2 h ago".
    pub(super) fn action_title(&self, index: usize) -> String {
        let Some(action) = self.actions.get(index) else {
            return "Quick action".to_string();
        };
        match self.action_results.get(&action.title) {
            Some(result) => format!(
                "{} · ran {}",
                action.title,
                action_results::ran_ago(result.ran_at, now_secs())
            ),
            None => action.title.clone(),
        }
    }

    /// Saves a completed run of the action, with its session for follow-ups.
    pub(super) fn record_action_result(
        &mut self,
        index: usize,
        answer: &str,
        plan: Option<DayPlan>,
    ) {
        let Some(action) = self.actions.get(index) else {
            return;
        };
        if answer.is_empty() && plan.is_none() {
            return;
        }
        let session = self
            .conversations
            .get_mut(&ChatId::Action(index))
            .session
            .clone();
        self.action_results
            .record(&action.title, answer, session.as_deref(), plan, now_secs());
        self.save_action_results();
        self.update_chat_title();
    }

    /// A follow-up continued the run's session: resume that one next time.
    pub(super) fn remember_action_session(&mut self, index: usize, session: &str) {
        let Some(action) = self.actions.get(index) else {
            return;
        };
        self.action_results.set_session(&action.title, session);
        self.save_action_results();
    }

    pub(super) fn save_action_results(&mut self) {
        let Some(path) = &self.results_path else {
            return;
        };
        if let Err(error) = self.action_results.save(path) {
            eprintln!("meow: {error}");
            let active = self.conversations.active().clone();
            self.error_in(&active, &error);
        }
    }
}

/// Saved results, or none plus the reason they could not be read. An
/// unreadable file is reported and left untouched.
pub(super) fn load_action_results(
    actions: &[QuickAction],
) -> (ActionResults, Option<std::path::PathBuf>, Option<String>) {
    let Some(path) = action_results::default_path() else {
        return (ActionResults::default(), None, None);
    };
    match ActionResults::load(&path) {
        Ok(mut results) => {
            // Morning planning became Today's planning: keep its last plan,
            // unless an action still goes by the old name.
            if !actions
                .iter()
                .any(|a| a.title == meow::actions::LEGACY_PLANNING)
            {
                results.rename(
                    meow::actions::LEGACY_PLANNING,
                    meow::actions::TODAYS_PLANNING,
                );
            }
            (results, Some(path), None)
        }
        Err(error) => (
            ActionResults::default(),
            None,
            Some(format!("{error}. Quick action results won't be saved.")),
        ),
    }
}
