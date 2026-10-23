//! The task list: adding, ticking, opening, and chatting about tasks.

use meow::conversations::ChatId;
use std::time::{Duration, Instant};

use meow::tasks::{self, FoundTask};

use super::plan::open_url;
use super::requests::{RunKind, STILL_WORKING};
use super::{App, now_secs};

/// How long a ticked task offers "draft reply" before it is done with no reply.
const REPLY_CHOICE_TIME: Duration = Duration::from_secs(5);

impl App {
    pub(super) fn add_task(&mut self) {
        let title = self.chat.tasks().take_input();
        if self.tasks.add_manual(&title, now_secs()) {
            self.tasks_changed();
        }
    }

    /// Merges tasks Claude reported, noting it in conversation `id`.
    pub(super) fn add_found_tasks(&mut self, id: &ChatId, found: &[FoundTask]) {
        if found.is_empty() {
            return;
        }
        let added = self.tasks.merge(found, now_secs());
        self.tasks_changed();
        if added > 0 {
            let plural = if added == 1 { "" } else { "s" };
            self.info_in(
                id,
                &format!("Added {added} task{plural} to your list (⌘T)."),
            );
        }
    }

    /// Ticking an open task asks whether to tell whoever asked: the row offers
    /// "draft reply" and "no reply" for a few seconds, then it is done with no
    /// reply. Ticking it again meanwhile takes it back.
    pub(super) fn toggle_task(&mut self, row: usize) {
        let Some(id) = self.chat.tasks().id_at(row) else {
            return;
        };
        if self
            .pending_done
            .as_ref()
            .is_some_and(|(pending, _)| *pending == id)
        {
            self.pending_done = None;
            self.tasks_changed();
            return;
        }
        if self.tasks.get(&id).is_some_and(|t| t.is_done()) {
            return self.toggle_task_id(&id);
        }
        self.finish_pending_done();
        self.pending_done = Some((id, Instant::now() + REPLY_CHOICE_TIME));
        self.tasks_changed();
    }

    /// Closes the reply choice once its time is up: done, no reply.
    pub(super) fn poll_pending_done(&mut self) {
        if self
            .pending_done
            .as_ref()
            .is_some_and(|(_, deadline)| Instant::now() >= *deadline)
        {
            self.finish_pending_done();
            self.tasks_changed();
        }
    }

    pub(super) fn no_done_reply(&mut self, row: usize) {
        if self.chat.tasks().id_at(row).is_some() {
            self.finish_pending_done();
            self.tasks_changed();
        }
    }

    /// Marks the task waiting for the reply choice done, if there is one.
    fn finish_pending_done(&mut self) {
        if let Some((id, _)) = self.pending_done.take() {
            self.tasks.set_done(&id, true, now_secs());
        }
    }

    pub(super) fn toggle_task_id(&mut self, id: &str) {
        let done = self.tasks.get(id).is_some_and(|t| t.is_done());
        self.tasks.set_done(id, !done, now_secs());
        self.tasks_changed();
    }

    /// Ticks the task, then asks Claude, in the task's conversation, to draft
    /// the reply telling whoever asked that it is done.
    pub(super) fn draft_done_reply(&mut self, row: usize) {
        let Some(id) = self.chat.tasks().id_at(row) else {
            return;
        };
        let chat = ChatId::Task(id.clone());
        if self.is_running(&chat) {
            self.chat_about_task_id(&id);
            self.info_in(&chat, STILL_WORKING);
            return;
        }
        self.pending_done = None;
        self.tasks.set_done(&id, true, now_secs());
        self.tasks_changed();
        self.chat_about_task_id(&id);
        let prompt = self.prompt_for(&chat, &tasks::done_reply_prompt());
        self.start_request(&chat, tasks::DONE_REPLY_REQUEST, &prompt, RunKind::Message);
    }

    /// Opens the task's link; a task without one opens its chat instead.
    pub(super) fn open_task(&mut self, row: usize) {
        let Some(id) = self.chat.tasks().id_at(row) else {
            return;
        };
        match self.tasks.get(&id).and_then(|task| task.url.clone()) {
            Some(url) => open_url(&url),
            None => self.chat_about_task_id(&id),
        }
    }

    pub(super) fn chat_about_task(&mut self, row: usize) {
        if let Some(id) = self.chat.tasks().id_at(row) {
            self.chat_about_task_id(&id);
        }
    }

    /// Opens the task's own conversation, introducing it the first time.
    pub(super) fn chat_about_task_id(&mut self, task_id: &str) {
        let Some(task) = self.tasks.get(task_id).cloned() else {
            return;
        };
        let id = ChatId::Task(task_id.to_string());
        self.show_conversation(id.clone());
        if self.conversations.current().transcript.blocks().is_empty() {
            self.info_in(&id, &format!("Chatting about: {}", task.title));
            if task.session.is_some() {
                self.info_in(&id, "Picking up where you left off.");
            }
        }
    }

    /// Prunes, saves and redraws the task list, and the plan whose items are tasks.
    pub(super) fn tasks_changed(&mut self) {
        self.tasks.prune(now_secs());
        let pending = self.pending_done.as_ref().map(|(id, _)| id.clone());
        self.chat.tasks().set_pending(pending);
        self.chat.render_tasks(&self.tasks, now_secs());
        self.render_plan();
        let Some(path) = &self.tasks_path else { return };
        if let Err(error) = self.tasks.save(path) {
            eprintln!("meow: {error}");
            let active = self.conversations.active().clone();
            self.error_in(&active, &error);
        }
    }
}
