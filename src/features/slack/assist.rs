//! Claude's private help with the open Slack conversation. Ask answers the
//! typed question from the loaded messages, with no tools; Autocomplete has
//! Opus research the reply with the user's own tools, skills and connectors,
//! read-only. Either way the answer shows in a panel only the user sees, and
//! the draft lands in the reply box for the user to edit and send themselves.
//! Ask hands scheduling requests to the calendar flow ([`super::slack_calendar`]).
//! Agent has Claude propose a coding agent for the conversation, started on the
//! user's go, whose output then shows in the same panel ([`super::agents`]).

use std::sync::mpsc::TryRecvError;

use meow::agent::{self, Proposal};
use meow::claude::{ClaudeEnv, ClaudeEvent, Run, STRONG_MODEL};
use meow::slack::assist::{self, Context, Reply};
use meow::slack::calendar;
use meow::superset::Project;

use crate::ui::slack_assist::AssistView;

use super::{Slack, now_secs};
use crate::app::feature::Host;

/// Asked when the user clicks Ask with nothing typed.
const DEFAULT_QUESTION: &str = "What is this conversation about, and what should I reply?";
/// Shown as the request of an Autocomplete run.
const AUTOCOMPLETE_LABEL: &str = "Autocomplete: research and draft the reply";
/// Asked when the user clicks Agent with nothing typed.
const DEFAULT_AGENT_REQUEST: &str = "Do the coding work this conversation asks of me.";

/// Which kind of help was asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Help {
    /// Answer the user's question from the conversation alone.
    #[default]
    Ask,
    /// Research with the user's tools, then draft the reply.
    Autocomplete,
    /// Propose a coding agent for what the conversation asks.
    Agent,
}

/// Where the help is.
#[derive(Default)]
pub(super) enum Stage {
    /// Ask or Autocomplete is running.
    #[default]
    Thinking,
    /// Claude's answer, or why there is none.
    Answered(Result<Reply, String>),
    /// Looking up who is in the conversation, for an invite.
    FindingPeople,
    /// Claude is planning the event, reading calendars only.
    Planning,
    /// An event waiting for the user's go.
    Proposed {
        note: String,
        proposal: calendar::Proposal,
    },
    /// The event is being created.
    Creating {
        note: String,
        proposal: calendar::Proposal,
    },
    /// A coding agent waiting for the user's go.
    AgentProposed { note: String, proposal: Proposal },
    /// The agent started; its state is in [`super::agents`].
    Agent,
}

#[derive(Default)]
pub(super) struct SlackAssist {
    pub(super) run: Option<Run>,
    /// The conversation asked about; the panel only shows while it is open.
    pub(super) channel: Option<String>,
    pub(super) help: Help,
    pub(super) question: String,
    /// What a research run has looked at so far.
    pub(super) steps: Vec<String>,
    pub(super) stage: Stage,
}

impl Slack {
    /// Asks Claude the typed question (or a default one) about the open conversation.
    pub(super) fn ask_slack(&mut self, host: &mut dyn Host) {
        let typed = self.page().take_input();
        let question = match typed.trim() {
            "" => DEFAULT_QUESTION.to_string(),
            question => question.to_string(),
        };
        self.start_help(Help::Ask, question, host);
    }

    /// Has Opus research the conversation with the user's tools and draft the reply.
    pub(super) fn autocomplete_slack(&mut self, host: &mut dyn Host) {
        self.start_help(Help::Autocomplete, AUTOCOMPLETE_LABEL.to_string(), host);
    }

    /// Has Claude propose a coding agent for the open conversation, for the
    /// typed request or what the conversation asks.
    pub(super) fn agent_slack(&mut self, host: &mut dyn Host) {
        let typed = self.page().take_input();
        let request = match typed.trim() {
            "" => DEFAULT_AGENT_REQUEST.to_string(),
            request => request.to_string(),
        };
        self.start_help(Help::Agent, request, host);
    }

    /// Starts the proposed agent: a new worktree in Superset, opened in the app.
    pub(super) fn start_slack_agent(&mut self, host: &mut dyn Host) {
        let Stage::AgentProposed { proposal, .. } = &self.slack_assist.stage else {
            return;
        };
        let Some(channel) = self.slack_assist.channel.clone() else {
            return;
        };
        let proposal = proposal.clone();
        let request = self.slack_assist.question.clone();
        self.slack_assist.stage = match host.launch_slack_agent(&channel, &request, &proposal) {
            Ok(()) => Stage::Agent,
            Err(error) => Stage::Answered(Err(error)),
        };
        self.render_slack_assist(host);
    }

    /// Shows the agent followed for the conversation just opened, if any.
    pub(super) fn show_slack_agent(&mut self, channel: &str, host: &mut dyn Host) {
        let Some(session) = host.slack_agent(channel) else {
            return;
        };
        self.slack_assist = SlackAssist {
            channel: Some(channel.to_string()),
            help: Help::Agent,
            question: session.request.clone(),
            stage: Stage::Agent,
            ..SlackAssist::default()
        };
        self.render_slack_assist(host);
    }

    /// Whether the panel shows the agent of conversation `channel`.
    /// Redraws Claude's panel when it follows a coding agent, which moved on.
    pub(super) fn redraw_shown_agent(&mut self, host: &mut dyn Host) {
        if matches!(self.slack_assist.stage, Stage::Agent) {
            self.render_slack_assist(host);
        }
    }

    fn start_help(&mut self, help: Help, question: String, host: &mut dyn Host) {
        let Some(context) = self.slack.assist_context() else {
            return;
        };
        let ctx = Context {
            title: &context.title,
            me: &context.me,
            lines: &context.lines,
            now: now_secs(),
            style: self.voice_guide_for_drafts(),
            can_schedule: host.calendar_tools().is_some(),
        };
        let started = claude_env(host).and_then(|env| {
            if help == Help::Agent {
                host.superset_ready()?;
            }
            start_run(env, help, &ctx, &question, host.projects())
        });
        if help == Help::Agent {
            host.discover_superset();
        }
        // Replacing the state drops (and so cancels) any request still running.
        self.slack_assist = SlackAssist {
            channel: Some(context.channel),
            help,
            question,
            ..SlackAssist::default()
        };
        match started {
            Ok(run) => self.slack_assist.run = Some(run),
            Err(error) => self.slack_assist.stage = Stage::Answered(Err(error)),
        }
        self.render_slack_assist(host);
    }

    /// Puts Claude's draft in the reply box; sending stays the user's call.
    pub(super) fn use_slack_draft(&mut self) {
        if let Stage::Answered(Ok(Reply {
            draft: Some(draft), ..
        })) = &self.slack_assist.stage
        {
            self.page().put_draft(draft);
        }
    }

    /// Hides the panel and stops any request still running.
    pub(super) fn close_slack_assist(&mut self) {
        self.slack_assist = SlackAssist::default();
        self.page().hide_assist();
    }

    pub(super) fn slack_assist_shown(&self) -> bool {
        self.page().assist_shown()
    }

    /// Collects the result of the run in flight and moves to the next stage.
    pub(super) fn poll_slack_assist(&mut self, host: &mut dyn Host) {
        loop {
            let Some(run) = &self.slack_assist.run else {
                return;
            };
            let outcome = match run.events.try_recv() {
                Ok(ClaudeEvent::Done {
                    is_error: false,
                    result,
                }) => Ok(result),
                Ok(ClaudeEvent::Done {
                    is_error: true,
                    result,
                }) => Err(result),
                Ok(ClaudeEvent::Failed(reason)) => Err(reason),
                Ok(ClaudeEvent::Tool(step)) => {
                    self.slack_assist.steps.push(step);
                    self.render_slack_assist(host);
                    continue;
                }
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("Claude stopped early.".to_string()),
            };
            self.slack_assist.run = None;
            return self.on_help_result(outcome, host);
        }
    }

    fn on_help_result(&mut self, outcome: Result<String, String>, host: &mut dyn Host) {
        let result = match outcome {
            Ok(result) => result,
            Err(error) => return self.answer(Err(error), host),
        };
        match std::mem::take(&mut self.slack_assist.stage) {
            Stage::Thinking
                if self.slack_assist.help == Help::Ask && calendar::wants_calendar(&result) =>
            {
                self.start_calendar(host)
            }
            Stage::Thinking if self.slack_assist.help == Help::Agent => {
                self.on_agent_proposal(&result, host)
            }
            Stage::Thinking => self.answer(Ok(assist::parse(&result)), host),
            Stage::Planning => self.on_planned(&result, host),
            Stage::Creating { proposal, .. } => self.on_created(&result, proposal, host),
            other => self.slack_assist.stage = other,
        }
    }

    /// Shows the proposed agent, or Claude's question when it could not write one.
    fn on_agent_proposal(&mut self, result: &str, host: &mut dyn Host) {
        match agent::extract(result) {
            (note, Some(proposal)) => {
                self.slack_assist.stage = Stage::AgentProposed { note, proposal };
                self.render_slack_assist(host);
            }
            (answer, None) => self.answer(
                Ok(Reply {
                    answer,
                    draft: None,
                }),
                host,
            ),
        }
    }

    /// Shows Claude's answer, and puts its draft in the reply box.
    pub(super) fn answer(&mut self, answer: Result<Reply, String>, host: &mut dyn Host) {
        self.slack_assist.stage = Stage::Answered(answer);
        self.render_slack_assist(host);
        self.fill_reply_with_draft();
    }

    /// Puts a fresh draft in the reply box, unless the user typed something
    /// there meanwhile ([ USE DRAFT ] still brings it in then).
    fn fill_reply_with_draft(&mut self) {
        if self.assist_is_on_screen() && self.page().input_is_empty() {
            self.use_slack_draft();
        }
    }

    /// Whether the help belongs to the conversation shown.
    pub(super) fn assist_is_on_screen(&self) -> bool {
        self.slack_assist.channel.is_some()
            && self.slack_assist.channel.as_deref() == self.slack.open_channel()
    }

    pub(super) fn render_slack_assist(&self, host: &mut dyn Host) {
        if !self.assist_is_on_screen() {
            return;
        }
        let state = &self.slack_assist;
        let question = state.question.as_str();
        let thinking = |label| AssistView::Thinking {
            question,
            label,
            steps: &state.steps,
        };
        let view = match &state.stage {
            Stage::Thinking if state.help == Help::Autocomplete => {
                thinking("Researching the reply…")
            }
            Stage::Thinking if state.help == Help::Agent => thinking("Writing the agent's prompt…"),
            Stage::AgentProposed { note, proposal } => AssistView::AgentProposal {
                question,
                note,
                project: &proposal.project,
                prompt: &proposal.prompt,
            },
            Stage::Agent => {
                let Some(session) = state.channel.as_deref().and_then(|c| host.slack_agent(c))
                else {
                    return self.page().hide_assist();
                };
                let status = session.status_line();
                let view = AssistView::Agent {
                    question,
                    title: &session.title,
                    steps: &session.steps,
                    status: &status,
                    message: session.messages.last().map(String::as_str),
                };
                return self.page().show_assist(&view);
            }
            Stage::Thinking => thinking("Reading the conversation…"),
            Stage::FindingPeople => thinking("Looking up who is in this conversation…"),
            Stage::Planning => thinking("Finding a time…"),
            Stage::Answered(Ok(reply)) => AssistView::Answer {
                question,
                answer: &reply.answer,
                draft: reply.draft.as_deref(),
            },
            Stage::Answered(Err(error)) => AssistView::Failed { question, error },
            Stage::Proposed { note, proposal } | Stage::Creating { note, proposal } => {
                let when = proposal.when();
                let view = AssistView::Event {
                    question,
                    note,
                    summary: &proposal.summary,
                    when: &when,
                    attendees: &proposal.attendees,
                    creating: matches!(state.stage, Stage::Creating { .. }),
                };
                return self.page().show_assist(&view);
            }
        };
        self.page().show_assist(&view);
    }
}

/// Ask runs tool-less on the everyday model; Autocomplete runs on Opus with
/// the user's tools, read-only; Agent writes the agent's prompt on Opus, with
/// no tools: it works from the conversation alone.
fn start_run(
    env: &ClaudeEnv,
    help: Help,
    ctx: &Context,
    question: &str,
    projects: &[Project],
) -> Result<Run, String> {
    match help {
        Help::Ask => Run::start_offline(env, &assist::prompt(ctx, question)),
        Help::Agent => Run::start_offline_on(
            env,
            &assist::agent_prompt(ctx, question, projects),
            STRONG_MODEL,
        ),
        Help::Autocomplete => {
            Run::start_read_only(env, &assist::autocomplete_prompt(ctx), STRONG_MODEL)
        }
    }
}

/// Claude, once discovered, or why it cannot run.
pub(super) fn claude_env(host: &dyn Host) -> Result<&ClaudeEnv, String> {
    host.claude_or_why()
}
