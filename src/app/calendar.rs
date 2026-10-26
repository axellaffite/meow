//! The calendar tools of the user's Claude Code, whichever connector it is:
//! at launch, a short `claude` run reports its tools and the calendar ones are
//! kept. Slack's calendar events, the agenda refresh and invitation answers
//! each run with only the tools they need from them.

use std::sync::mpsc::TryRecvError;

use meow::claude::{ClaudeEvent, Run};
use meow::slack::calendar::CalendarTools;

use super::App;

/// The prompt of the run that only reports the tools; it is stopped before
/// it answers.
const PROBE_PROMPT: &str = "Reply with: ok";

/// The calendar tools of the user's Claude Code, once known.
#[derive(Default)]
pub(super) struct CalendarState {
    tools: Option<CalendarTools>,
    probe: Option<Run>,
    probed: bool,
}

impl CalendarState {
    pub(super) fn tools(&self) -> Option<&CalendarTools> {
        self.tools.as_ref()
    }
}

impl App {
    /// Finds the calendar tools once Claude is ready.
    pub(super) fn poll_calendar_probe(&mut self) {
        if self.calendar.probed {
            return;
        }
        if self.calendar.probe.is_none() {
            let Some(Ok(env)) = &self.claude else {
                return;
            };
            // No tools allowed: the run only has to start and list them.
            match Run::start_restricted(env, PROBE_PROMPT, &env.model, &[]) {
                Ok(run) => self.calendar.probe = Some(run),
                Err(error) => {
                    eprintln!("meow: could not list Claude's tools: {error}");
                    self.calendar.probed = true;
                }
            }
            return;
        }
        loop {
            let Some(probe) = &self.calendar.probe else {
                return;
            };
            match probe.events.try_recv() {
                Ok(ClaudeEvent::Tools(tools)) => self.calendar.tools = CalendarTools::find(&tools),
                Ok(ClaudeEvent::Done { .. } | ClaudeEvent::Failed(_))
                | Err(TryRecvError::Disconnected) => {}
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return,
            }
            // Dropping the run stops it.
            self.calendar.probe = None;
            self.calendar.probed = true;
            return;
        }
    }
}
