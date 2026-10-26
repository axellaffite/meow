//! Keeps today's plan agenda current: every few minutes, a quiet Claude run
//! limited to the calendar's read tools reports today's events, and they
//! replace the agenda of each plan made today. It never blocks the chat.

#[cfg(test)]
mod tests;

use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use meow::actions::Layout;
use meow::activity::{Kind, Link, Note};
use meow::agenda::{self, Change};
use meow::claude::{ClaudeEvent, Run};
use meow::clock;
use meow::plan::Event;

use crate::app::feature::{Feature, Host};

const DEFAULT_MINUTES: u64 = 15;
/// The first refresh waits a little after launch, so startup stays quick.
const FIRST_DELAY: Duration = Duration::from_secs(30);

pub(crate) struct AgendaRefresh {
    /// `None` when refreshes are turned off (`MEOW_AGENDA_MINUTES=0`).
    interval: Option<Duration>,
    next_at: Instant,
    run: Option<Run>,
}

impl AgendaRefresh {
    /// Reads `MEOW_AGENDA_MINUTES` (default 15, 0 turns refreshes off).
    pub(crate) fn from_env() -> Self {
        let minutes = std::env::var("MEOW_AGENDA_MINUTES")
            .ok()
            .and_then(|m| m.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_MINUTES);
        Self::new((minutes > 0).then(|| Duration::from_secs(minutes * 60)))
    }

    fn new(interval: Option<Duration>) -> Self {
        Self {
            interval,
            next_at: Instant::now() + FIRST_DELAY,
            run: None,
        }
    }

    fn start(&mut self, interval: Duration, host: &mut dyn Host) {
        if plans_to_refresh(host, interval).is_empty() {
            return;
        }
        let (Some(env), Some(tools)) = (host.claude(), host.calendar_tools()) else {
            return;
        };
        match Run::start_restricted(env, &agenda::prompt(now_secs()), &env.model, &tools.read) {
            Ok(run) => self.run = Some(run),
            Err(error) => eprintln!("meow: could not refresh the agenda: {error}"),
        }
    }

    fn collect(&mut self, host: &mut dyn Host) {
        loop {
            let Some(run) = &self.run else { return };
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
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("the refresh stopped early".to_string()),
            };
            self.run = None;
            return self.finish(outcome.and_then(|answer| agenda::parse(&answer)), host);
        }
    }

    fn finish(&mut self, outcome: Result<Vec<Event>, String>, host: &mut dyn Host) {
        let events = match outcome {
            Ok(events) => events,
            Err(error) => return eprintln!("meow: agenda refresh failed: {error}"),
        };
        // Zero: the plan made at start-up counts as fresh, never as stale.
        let titles = plans_to_refresh(host, Duration::ZERO);
        let Some(first) = titles.first().cloned() else {
            return;
        };
        let agenda_of = |host: &dyn Host| {
            host.last_result(&first)
                .and_then(|(_, plan)| plan)
                .map(|plan| plan.agenda.clone())
                .unwrap_or_default()
        };
        let old = agenda_of(host);
        let mut changed = false;
        for title in &titles {
            changed |= host.set_plan_agenda(title, events.clone());
        }
        if !changed {
            return;
        }
        host.plans_changed();
        // Compared once cleaned, the way the plan keeps them.
        let new = agenda_of(host);
        let now_minutes = clock::local(now_secs()).map_or(0, |t| t.minutes_of_day());
        for change in agenda::changes(&old, &new, now_minutes) {
            host.record(calendar_note(change, &first));
        }
    }
}

impl Feature for AgendaRefresh {
    /// Starts a refresh when one is due, and collects the one in flight.
    fn poll(&mut self, host: &mut dyn Host) {
        if self.run.is_some() {
            self.collect(host);
        } else if let Some(interval) = self.interval
            && Instant::now() >= self.next_at
        {
            self.next_at = Instant::now() + interval;
            self.start(interval, host);
        }
    }

    fn is_busy(&self) -> bool {
        self.run.is_some()
    }
}

/// Titles of the plan actions whose plan was made today, and not within the
/// last `interval`.
fn plans_to_refresh(host: &dyn Host, interval: Duration) -> Vec<String> {
    let now = now_secs();
    let Some(today) = clock::local(now) else {
        return Vec::new();
    };
    host.quick_actions()
        .iter()
        .filter(|action| action.layout == Layout::Plan)
        .filter(|action| match host.last_result(&action.title) {
            Some((ran_at, Some(_))) => {
                let fresh = now.saturating_sub(ran_at) < interval.as_secs();
                let made_today = clock::local(ran_at).is_some_and(|at| at.same_day(&today));
                made_today && !fresh
            }
            _ => false,
        })
        .map(|action| action.title.clone())
        .collect()
}

/// The Activity item for one agenda change, linking to the plan.
fn calendar_note(change: Change, plan: &str) -> Note {
    let at = |start: &Option<String>| start.clone().unwrap_or_else(|| "all day".to_string());
    let (title, detail) = match change {
        Change::Added(event) => (format!("New event: {}", event.title), at(&event.start)),
        Change::Cancelled(event) => (
            format!("Cancelled: {}", event.title),
            format!("was at {}", at(&event.start)),
        ),
        Change::Moved { event, from } => (
            format!("Moved: {}", event.title),
            format!("{} → {}", at(&from), at(&event.start)),
        ),
        Change::Invited(event) => (
            format!("Invitation: {}", event.title),
            format!("{} · waiting for your answer", at(&event.start)),
        ),
    };
    Note {
        kind: Kind::Calendar,
        title,
        detail: Some(detail),
        link: Some(Link::Plan {
            action: plan.to_string(),
        }),
        sources: Vec::new(),
        key: None,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
