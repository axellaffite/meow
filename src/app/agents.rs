//! Coding agents started from the chat (`/agent …`) or from a Slack
//! conversation. Meow's Claude writes a [`Proposal`]; the agent then runs in a
//! new Superset worktree, opened in the app, and what it writes comes back to
//! where it was asked: the chat turn, or the conversation's private panel.
//! Nothing is posted to Slack, and the workspaces are the user's to keep or
//! delete in Superset.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use meow::agent::{self, Outcome, Proposal};
use meow::agent_run::{AgentEvent, AgentRun, Launch, Pace};
use meow::conversations::ChatId;
use meow::superset::{Project, Superset};

use super::requests::RunKind;
use super::{App, now_secs};

type Discovery = Result<(Superset, Vec<Project>), String>;

/// Where an agent was asked for, and so where its output goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The chat turn of the `/agent` request (a transcript entry index).
    Chat {
        chat: ChatId,
        turn: usize,
    },
    Slack {
        channel: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Starting,
    Running,
    Finished(Outcome),
    Quiet,
    Failed(String),
}

/// What an event changes in the conversation that asked.
enum Update {
    Step(String),
    Message { text: String, grown: bool },
    Ended,
    Nothing,
}

pub(crate) struct AgentSession {
    pub(crate) origin: Origin,
    /// `None` once the agent is no longer followed.
    run: Option<AgentRun>,
    /// "project · workspace".
    pub(crate) title: String,
    /// What the user asked, shown above the agent's output.
    pub(crate) request: String,
    pub(crate) steps: Vec<String>,
    pub(crate) messages: Vec<String>,
    pub(crate) status: Status,
}

impl AgentSession {
    fn apply(&mut self, event: AgentEvent) -> Update {
        let update = match event {
            AgentEvent::Created(workspace) => Update::Step(format!(
                "Created workspace {} on {}",
                workspace.name, workspace.branch
            )),
            AgentEvent::OpenFailed(error) => {
                Update::Step(format!("Could not show it in Superset: {error}"))
            }
            AgentEvent::Started => {
                self.status = Status::Running;
                Update::Step(format!("Started {} in it", agent::AGENT))
            }
            AgentEvent::Message { index, text } => self.message(index, text),
            AgentEvent::Finished(outcome) => self.end(Status::Finished(outcome)),
            AgentEvent::Quiet => self.end(Status::Quiet),
            AgentEvent::Failed(error) => self.end(Status::Failed(error)),
        };
        if let Update::Step(step) = &update {
            self.steps.push(step.clone());
        }
        update
    }

    fn message(&mut self, index: usize, text: String) -> Update {
        let last = self.messages.len().checked_sub(1);
        if index >= self.messages.len() {
            self.messages.push(text.clone());
            Update::Message { text, grown: false }
        } else if Some(index) == last {
            self.messages[index] = text.clone();
            Update::Message { text, grown: true }
        } else {
            // Only the latest message can still change.
            Update::Nothing
        }
    }

    fn end(&mut self, status: Status) -> Update {
        self.status = status;
        self.run = None;
        Update::Ended
    }

    /// One line on where the agent is, for the Slack panel.
    pub(crate) fn status_line(&self) -> String {
        match &self.status {
            Status::Starting => "Creating the workspace…".to_string(),
            Status::Running => "The agent is working…".to_string(),
            Status::Finished(Outcome::Done(summary)) => format!("Done: {summary}"),
            Status::Finished(Outcome::Blocked(need)) => format!("Blocked: {need}"),
            Status::Quiet => quiet_line(),
            Status::Failed(error) => format!("Failed: {error}"),
        }
    }
}

fn quiet_line() -> String {
    let minutes = Pace::default().quiet_after.as_secs() / 60;
    format!("No news from the agent for {minutes} min: see its workspace in Superset.")
}

#[derive(Default)]
pub(super) struct AgentsState {
    superset: Option<Result<Superset, String>>,
    projects: Vec<Project>,
    discovery: Option<Receiver<Discovery>>,
    sessions: Vec<AgentSession>,
    /// Counts the agents' updates, so a page showing one knows to redraw.
    revision: u64,
}

impl AgentsState {
    /// An agent is being started or followed.
    pub(super) fn is_running(&self) -> bool {
        self.sessions.iter().any(|session| session.run.is_some())
    }
}

impl App {
    /// Looks for `superset` and its projects in the background, once Claude's
    /// PATH is known; again later to pick up new projects.
    pub(super) fn discover_superset(&mut self) {
        if self.agents.discovery.is_some() {
            return;
        }
        let Some(Ok(env)) = &self.claude else {
            return;
        };
        let path_var = env.path_var.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let found = Superset::discover(&path_var)
                .and_then(|superset| superset.projects().map(|projects| (superset, projects)));
            let _ = tx.send(found);
        });
        self.agents.discovery = Some(rx);
    }

    pub(super) fn poll_agents(&mut self) {
        self.poll_superset_discovery();
        for index in 0..self.agents.sessions.len() {
            while let Some(event) = self.next_agent_event(index) {
                let update = self.agents.sessions[index].apply(event);
                self.show_agent_update(index, update);
            }
        }
        // A chat agent lives on in its turn; a Slack one stays for its panel.
        self.agents
            .sessions
            .retain(|s| s.run.is_some() || matches!(s.origin, Origin::Slack { .. }));
    }

    fn poll_superset_discovery(&mut self) {
        let Some(discovery) = &self.agents.discovery else {
            return;
        };
        let found = match discovery.try_recv() {
            Ok(found) => found,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Could not look for Superset.".to_string()),
        };
        self.agents.discovery = None;
        match found {
            Ok((superset, projects)) => {
                self.agents.superset = Some(Ok(superset));
                self.agents.projects = projects;
            }
            // A failed refresh keeps what an earlier look found.
            Err(error) if !matches!(self.agents.superset, Some(Ok(_))) => {
                eprintln!("meow: {error}");
                self.agents.superset = Some(Err(error));
            }
            Err(error) => eprintln!("meow: {error}"),
        }
    }

    fn next_agent_event(&mut self, index: usize) -> Option<AgentEvent> {
        let session = &mut self.agents.sessions[index];
        match session.run.as_ref()?.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(AgentEvent::Failed(
                "Meow lost track of the agent.".to_string(),
            )),
        }
    }

    /// Whether agents can be proposed now, or why not.
    pub(super) fn superset_ready(&self) -> Result<(), String> {
        match &self.agents.superset {
            Some(Ok(_)) if self.agents.projects.is_empty() => {
                Err("Superset has no projects on this machine yet.".to_string())
            }
            Some(Ok(_)) => Ok(()),
            Some(Err(error)) => Err(format!("Superset is not available: {error}")),
            None => Err("Still looking for Superset; try again in a moment.".to_string()),
        }
    }

    pub(super) fn projects(&self) -> &[Project] {
        &self.agents.projects
    }

    /// `/agent <request>` in a conversation: Claude writes the agent's prompt
    /// (looking things up, read-only), then the agent starts.
    pub(super) fn request_agent_in_chat(&mut self, id: &ChatId, label: &str, request: &str) {
        if request.is_empty() {
            return self.info_in(id, "Type what the agent should do after /agent.");
        }
        if let Err(error) = self.superset_ready() {
            return self.error_in(id, &error);
        }
        let prompt = agent::chat_prompt(request, &self.agents.projects);
        self.start_request(id, label, &prompt, RunKind::Agent);
        self.discover_superset();
    }

    /// Claude proposed an agent in turn `turn` of chat `id`: shows the proposal
    /// in the turn's steps, and starts the agent, whose output follows there.
    pub(super) fn hand_over_to_agent(&mut self, id: &ChatId, shown: &str, proposal: Proposal) {
        let Some(turn) = self.transcript(id).running_turn_index() else {
            return;
        };
        let transcript = self.transcript(id);
        if !shown.is_empty() {
            transcript.text_at(turn, shown);
        }
        transcript.text_at(
            turn,
            &format!(
                "**Agent in {}:** {}\n\n{}",
                proposal.project, proposal.name, proposal.prompt
            ),
        );
        let origin = Origin::Chat {
            chat: id.clone(),
            turn,
        };
        let request = proposal.name.clone();
        if let Err(error) = self.launch_agent(origin, &request, &proposal) {
            self.transcript(id).finish_turn_at(turn, "");
            self.error_in(id, &error);
        }
        self.refresh(id, true);
    }

    /// Starts `proposal`'s agent in a new worktree of its project.
    pub(super) fn launch_agent(
        &mut self,
        origin: Origin,
        request: &str,
        proposal: &Proposal,
    ) -> Result<(), String> {
        self.superset_ready()?;
        let Some(Ok(superset)) = &self.agents.superset else {
            return Err("Superset is not available.".to_string());
        };
        let project = agent::find_project(&self.agents.projects, &proposal.project)
            .ok_or_else(|| format!("Superset has no project named {}.", proposal.project))?;
        let launch = Launch {
            project_id: project.id.clone(),
            name: proposal.name.clone(),
            branch: agent::branch_name(&proposal.name, now_secs()),
            prompt: agent::agent_prompt(&proposal.prompt),
        };
        let session = AgentSession {
            title: format!("{} · {}", project.name, proposal.name),
            run: Some(AgentRun::start(superset.clone(), launch, Pace::default())),
            origin,
            request: request.to_string(),
            steps: Vec::new(),
            messages: Vec::new(),
            status: Status::Starting,
        };
        // One agent per Slack conversation is followed; an older one keeps
        // running in Superset.
        if let Origin::Slack { channel } = &session.origin {
            let channel = channel.clone();
            self.agents
                .sessions
                .retain(|s| !matches!(&s.origin, Origin::Slack { channel: c } if *c == channel));
        }
        self.agents.sessions.push(session);
        Ok(())
    }

    /// Stops following the agents of chat `id`, before its transcript is
    /// reset (their turn indices would point into the new one). The agents
    /// keep running in Superset. Returns how many there were.
    pub(super) fn detach_chat_agents(&mut self, id: &ChatId) -> usize {
        let before = self.agents.sessions.len();
        self.agents
            .sessions
            .retain(|s| !matches!(&s.origin, Origin::Chat { chat, .. } if chat == id));
        before - self.agents.sessions.len()
    }

    /// The agent followed for Slack conversation `channel`.
    pub(super) fn slack_agent(&self, channel: &str) -> Option<&AgentSession> {
        self.agents
            .sessions
            .iter()
            .rev()
            .find(|s| matches!(&s.origin, Origin::Slack { channel: c } if c == channel))
    }

    /// Changes each time an agent moves on.
    pub(super) fn agents_revision(&self) -> u64 {
        self.agents.revision
    }

    fn show_agent_update(&mut self, index: usize, update: Update) {
        self.agents.revision += 1;
        let session = &self.agents.sessions[index];
        match session.origin.clone() {
            Origin::Chat { chat, turn } => {
                let status = session.status.clone();
                self.show_in_chat(&chat, turn, update, &status);
            }
            // Its Slack conversation's panel redraws as the revision changes.
            Origin::Slack { .. } => {}
        }
    }

    fn show_in_chat(&mut self, chat: &ChatId, turn: usize, update: Update, status: &Status) {
        let transcript = self.transcript(chat);
        match update {
            Update::Step(step) => transcript.tool_at(turn, &step),
            Update::Message { text, grown: true } => transcript.replace_text_at(turn, &text),
            Update::Message { text, grown: false } => transcript.text_at(turn, &text),
            Update::Nothing => return,
            Update::Ended => {
                transcript.finish_turn_at(turn, "");
                match status {
                    Status::Finished(Outcome::Done(summary)) => {
                        transcript.info(&format!("✓ Agent done: {summary}"))
                    }
                    Status::Finished(Outcome::Blocked(need)) => {
                        transcript.info(&format!("Agent blocked: {need}"))
                    }
                    Status::Quiet => transcript.info(&quiet_line()),
                    Status::Failed(error) => transcript.error(&format!("Agent failed: {error}")),
                    Status::Starting | Status::Running => {}
                }
                if !self.chat.is_visible() || self.conversations.active() != chat {
                    self.unread = true;
                    self.unread_chat = chat.clone();
                }
            }
        }
        self.refresh(chat, true);
    }
}
