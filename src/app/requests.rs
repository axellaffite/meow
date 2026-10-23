//! Sending requests to Claude and routing what comes back to the conversation
//! that asked, even if another one is on screen by then. Each conversation
//! runs one request at a time; different conversations run side by side.

use std::collections::HashMap;
use std::sync::mpsc::TryRecvError;

use meow::claude::{ClaudeEvent, Run};
use meow::conversations::ChatId;
use meow::{agent, plan, routines, tasks};

use super::update::Asked;
use super::{App, now_secs};

/// Whether a request is a quick action's run (its answer is saved as the
/// action's result), a message in a conversation, or an `/agent` request
/// (Claude writes a coding agent's prompt, read-only, and the agent starts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    Message,
    Action,
    Agent,
}

const AGENT_COMMAND: &str = "/agent";
const UPDATE_COMMAND: &str = "/update";

pub(super) const STILL_WORKING: &str = "Still working on the last request (Esc to cancel).";

/// A request in flight; its conversation is its key in `App::runs`.
pub struct ActiveRun {
    run: Run,
    kind: RunKind,
    /// Running sub-agents, by launching tool call id.
    agents: HashMap<String, String>,
}

impl App {
    pub(super) fn submit(&mut self) {
        let text = self.chat.chat().take_input();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if text == "/new" {
            self.new_conversation();
            return;
        }
        let id = self.conversations.active().clone();
        if text == UPDATE_COMMAND {
            return self.check_for_updates(Asked::Chat(id));
        }
        if let Some(request) = agent_request(text) {
            return self.request_agent_in_chat(&id, text, request);
        }
        // Claude reports "do this later" requests instead of doing them now,
        // and sees the scheduled tasks there are, to change or restart them.
        let prompt = format!(
            "{}\n\n{}",
            self.prompt_for(&id, text),
            routines::instruction(now_secs(), &self.routines.routines)
        );
        self.start_request(&id, text, &prompt, RunKind::Message);
    }

    /// A task's conversation starts by telling Claude which task it is about.
    pub(super) fn prompt_for(&self, id: &ChatId, text: &str) -> String {
        let fresh = self.conversations.current().session.is_none();
        match id {
            ChatId::Task(task) if fresh => match self.tasks.get(task) {
                Some(task) => tasks::chat_prompt(task, text),
                None => text.to_string(),
            },
            _ => text.to_string(),
        }
    }

    /// Sends `prompt` in conversation `id`, showing `label` as the user's line.
    pub(super) fn start_request(&mut self, id: &ChatId, label: &str, prompt: &str, kind: RunKind) {
        if self.runs.contains_key(id) {
            self.info_in(id, STILL_WORKING);
            return;
        }
        let env = match &self.claude {
            Some(Ok(env)) => env.clone(),
            Some(Err(error)) => {
                let error = error.clone();
                self.error_in(id, &error);
                return;
            }
            None => {
                self.info_in(
                    id,
                    "Still looking for the claude command, try again in a second.",
                );
                return;
            }
        };
        let session = self.conversations.get_mut(id).session.clone();
        self.transcript(id).start_turn(label);
        let started = match kind {
            RunKind::Agent => Run::start_planning(&env, prompt, session.as_deref()),
            RunKind::Message | RunKind::Action => Run::start(&env, prompt, session.as_deref()),
        };
        match started {
            Ok(run) => {
                self.runs.insert(
                    id.clone(),
                    ActiveRun {
                        run,
                        kind,
                        agents: HashMap::new(),
                    },
                );
                self.sync_busy();
                self.refresh(id, true);
            }
            Err(error) => {
                self.transcript(id).finish_turn("");
                self.error_in(id, &error);
            }
        }
    }

    /// Whether conversation `id` has a request in flight.
    pub(super) fn is_running(&self, id: &ChatId) -> bool {
        self.runs.contains_key(id)
    }

    /// Stops conversation `id`'s request; dropping the run kills Claude.
    pub(super) fn cancel_run(&mut self, id: &ChatId) {
        if self.runs.remove(id).is_none() {
            return;
        }
        self.conversations.get_mut(id).pending_session = None;
        self.sync_busy();
        self.transcript(id).finish_turn("");
        self.info_in(id, "Cancelled.");
    }

    /// The "MEOW ···" title shows while any conversation waits on Claude.
    fn sync_busy(&self) {
        self.chat.set_busy(!self.runs.is_empty());
    }

    /// Starts the conversation on screen over, forgetting its Claude session.
    pub(super) fn new_conversation(&mut self) {
        let id = self.conversations.active().clone();
        if self.runs.remove(&id).is_some() {
            self.sync_busy();
        }
        let detached = self.detach_chat_agents(&id);
        self.conversations.get_mut(&id).reset();
        match &id {
            ChatId::Task(task) => {
                self.tasks.set_session(task, None);
                self.tasks_changed();
            }
            ChatId::Action(index) => self.restart_action_conversation(*index),
            ChatId::Main => {}
        }
        self.unread = false;
        self.info("New conversation.");
        if detached > 0 {
            self.info("The agent keeps running in its Superset workspace.");
        }
    }

    pub(super) fn poll_claude(&mut self) {
        let ids: Vec<ChatId> = self.runs.keys().cloned().collect();
        for id in ids {
            self.poll_run(&id);
        }
    }

    fn poll_run(&mut self, id: &ChatId) {
        loop {
            let Some(active) = self.runs.get(id) else {
                return;
            };
            match active.run.events.try_recv() {
                Ok(event) => self.on_claude_event(id.clone(), event),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.finish_run(id, "");
                    return;
                }
            }
        }
    }

    fn on_claude_event(&mut self, id: ChatId, event: ClaudeEvent) {
        match event {
            ClaudeEvent::Session(session) => {
                self.conversations.get_mut(&id).pending_session = Some(session);
            }
            // The chat does not need the tool list.
            ClaudeEvent::Tools(_) => {}
            ClaudeEvent::Text(text) => {
                let (shown, found) = tasks::extract(&text);
                let (shown, _) = plan::extract(&shown);
                let (shown, _) = agent::extract(&shown);
                let (shown, _) = routines::extract(&shown);
                self.add_found_tasks(&id, &found);
                if !shown.is_empty() {
                    self.transcript(&id).text(&shown);
                    self.refresh(&id, true);
                }
            }
            ClaudeEvent::Tool(summary) => {
                self.transcript(&id).tool(&summary);
                self.refresh(&id, true);
            }
            ClaudeEvent::AgentStarted {
                id: agent,
                description,
            } => {
                if let Some(active) = self.runs.get_mut(&id) {
                    active.agents.insert(agent, description);
                }
            }
            ClaudeEvent::AgentFinished { id: agent } => {
                let description = self.runs.get_mut(&id).and_then(|a| a.agents.remove(&agent));
                if let Some(description) = description {
                    self.transcript(&id).tool(&format!("✓ {description}"));
                    self.refresh(&id, true);
                }
            }
            ClaudeEvent::Done {
                is_error: true,
                result,
            } => {
                let message = if result.is_empty() {
                    "Claude reported an error.".to_string()
                } else {
                    result
                };
                self.finish_run(&id, "");
                self.error_in(&id, &message);
            }
            ClaudeEvent::Done {
                is_error: false,
                result,
            } => {
                let kind = self.runs.get(&id).map_or(RunKind::Message, |run| run.kind);
                self.remember_session(&id, kind);
                if kind == RunKind::Agent
                    && let (shown, Some(proposal)) = agent::extract(&result)
                {
                    // The turn stays open: the agent's output follows in it.
                    self.runs.remove(&id);
                    self.sync_busy();
                    return self.hand_over_to_agent(&id, &shown, proposal);
                }
                let (shown, found) = tasks::extract(&result);
                let (shown, day_plan) = plan::extract(&shown);
                let (shown, schedule) = routines::extract(&shown);
                self.add_found_tasks(&id, &found);
                if let Some(day_plan) = &day_plan {
                    self.add_found_tasks(&id, &day_plan.tasks());
                }
                let new_plan = day_plan.is_some();
                if let (RunKind::Action, ChatId::Action(index)) = (kind, &id) {
                    self.record_action_result(*index, &shown, day_plan);
                }
                self.finish_run(&id, &shown);
                if let Some(spec) = schedule {
                    self.apply_routine(&id, &spec);
                }
                if new_plan && self.conversations.active() == &id {
                    self.show_conversation(id);
                }
            }
            ClaudeEvent::Failed(reason) => {
                self.finish_run(&id, "");
                self.error_in(&id, &reason);
            }
        }
    }

    /// Keeps the completed request's session so follow-ups resume it; a task's
    /// session is saved with the task, a follow-up's with its action's result.
    fn remember_session(&mut self, id: &ChatId, kind: RunKind) {
        let Some(session) = self
            .conversations
            .get_mut(id)
            .complete()
            .map(str::to_string)
        else {
            return;
        };
        match id {
            ChatId::Task(task) => {
                self.tasks.set_session(task, Some(&session));
                self.tasks_changed();
            }
            // An action run's session is saved with its result instead.
            ChatId::Action(index) if kind != RunKind::Action => {
                self.remember_action_session(*index, &session);
            }
            ChatId::Action(_) | ChatId::Main => {}
        }
    }

    /// Ends conversation `id`'s request; `result` is the answer if no text was streamed.
    fn finish_run(&mut self, id: &ChatId, result: &str) {
        if self.runs.remove(id).is_none() {
            return;
        }
        self.conversations.get_mut(id).pending_session = None;
        self.transcript(id).finish_turn(result);
        self.refresh(id, true);
        self.sync_busy();
        if !self.chat.is_visible() || self.conversations.active() != id {
            self.unread = true;
            self.unread_chat = id.clone();
        }
    }
}

/// What follows `/agent `, when `text` is an agent request.
fn agent_request(text: &str) -> Option<&str> {
    let rest = text.strip_prefix(AGENT_COMMAND)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_requests_are_recognized() {
        assert_eq!(
            agent_request("/agent fix the export"),
            Some("fix the export")
        );
        assert_eq!(agent_request("/agent"), Some(""));
        assert_eq!(agent_request("/agents"), None);
        assert_eq!(agent_request("start an /agent"), None);
    }
}
