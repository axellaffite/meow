//! The background check of tasks: every few minutes, a quiet Claude session
//! removes resolved tasks and adds new ones. It runs next to the chat, never
//! blocks it, and reports in the Tasks header and in Activity.

#[cfg(test)]
mod tests;

use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use meow::activity::{self, Kind, Link, Note};
use meow::claude::{ClaudeEvent, Run};
use meow::clock;
use meow::source::Source;
use meow::sync;

use crate::app::feature::{Feature, Host};

const DEFAULT_MINUTES: u64 = 5;
/// The first check waits a little after launch, so startup stays quick.
const FIRST_DELAY: Duration = Duration::from_secs(20);
/// How often "checked N min ago" is refreshed. A failed check is retried at
/// the normal pace, so a broken connector does not burn usage.
const STATUS_REFRESH: Duration = Duration::from_secs(30);

pub(crate) struct BackgroundCheck {
    /// `None` when checks are turned off (`MEOW_SYNC_MINUTES=0`).
    interval: Option<Duration>,
    next_at: Instant,
    run: Option<Run>,
    started_at: u64,
    /// Time of the last successful check, for "checked 3 min ago" and the
    /// next check's look-back window.
    last_ok: Option<u64>,
    last_failed: bool,
    status_shown_at: Instant,
}

impl BackgroundCheck {
    /// Reads `MEOW_SYNC_MINUTES` (default 5, 0 turns checks off).
    pub(crate) fn from_env() -> Self {
        let minutes = std::env::var("MEOW_SYNC_MINUTES")
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
            started_at: 0,
            last_ok: None,
            last_failed: false,
            status_shown_at: Instant::now(),
        }
    }

    fn is_due(&self) -> bool {
        self.interval.is_some() && Instant::now() >= self.next_at
    }

    fn start(&mut self, host: &mut dyn Host) {
        let now = now_secs();
        let prompt = {
            let tasks = host.tasks();
            sync::prompt(&tasks.open(), &tasks.recently_done(), self.last_ok, now)
        };
        let Some(env) = host.claude() else {
            return;
        };
        match Run::start(env, &prompt, None) {
            Ok(run) => {
                self.run = Some(run);
                self.started_at = now;
            }
            Err(error) => self.finish(Err(error), host),
        }
        self.show_status(host);
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
                Err(TryRecvError::Disconnected) => Err("the check stopped early".to_string()),
            };
            return self.finish(outcome, host);
        }
    }

    fn finish(&mut self, outcome: Result<String, String>, host: &mut dyn Host) {
        self.run = None;
        if let Some(interval) = self.interval {
            self.next_at = Instant::now() + interval;
        }
        match outcome.and_then(|answer| sync::parse(&answer)) {
            Ok(report) => {
                let now = now_secs();
                let tasks = host.tasks();
                let closed_tasks: Vec<(String, Source)> = report
                    .done
                    .iter()
                    .filter_map(|id| tasks.get(id))
                    .filter(|task| !task.is_done())
                    .map(|task| (task.title.clone(), Source::parse(&task.source)))
                    .collect();
                let new_found = sync::fresh(&report, tasks);
                let new_tasks: Vec<(&str, Source)> = new_found
                    .iter()
                    .map(|found| {
                        let source = found.source.as_deref().map_or(Source::Other, Source::parse);
                        (found.title.as_str(), source)
                    })
                    .collect();
                let closed = tasks.close_resolved(&report.done, now);
                let added = tasks.merge(&new_found, now);
                if closed + added > 0 {
                    host.tasks_changed();
                }
                announce(host, &new_tasks, &closed_tasks);
                // New things are looked for from when this check started.
                self.last_ok = Some(self.started_at);
                self.last_failed = false;
            }
            Err(error) => {
                eprintln!("meow: task check failed: {error}");
                self.last_failed = true;
            }
        }
        self.show_status(host);
    }

    /// "checked at 09:40" after "Tasks" in the Tasks header.
    fn show_status(&mut self, host: &mut dyn Host) {
        self.status_shown_at = Instant::now();
        let status = if self.interval.is_none() {
            None
        } else if self.run.is_some() {
            Some("checking…".to_string())
        } else if self.last_failed {
            Some("last check failed".to_string())
        } else {
            self.last_ok
                .and_then(clock::local)
                .map(|at| format!("checked at {:02}:{:02}", at.hour, at.minute))
        };
        host.set_tasks_status(status.as_deref());
    }
}

impl Feature for BackgroundCheck {
    /// Starts a check when one is due, and collects the one in flight.
    fn poll(&mut self, host: &mut dyn Host) {
        if self.run.is_some() {
            self.collect(host);
        } else if self.is_due() && host.claude().is_some() {
            self.start(host);
        }
        if self.status_shown_at.elapsed() >= STATUS_REFRESH {
            self.show_status(host);
        }
    }

    fn is_busy(&self) -> bool {
        self.run.is_some()
    }
}

/// One Activity item for the tasks a check added, one for those it closed,
/// each with the sources they come from.
fn announce(host: &mut dyn Host, added: &[(&str, Source)], closed: &[(String, Source)]) {
    let count = |n: usize, what: &str| match n {
        1 => format!("1 {what}"),
        n => format!("{n} {what}s"),
    };
    if !added.is_empty() {
        let titles: Vec<&str> = added.iter().map(|(title, _)| *title).collect();
        host.record(Note {
            kind: Kind::Tasks,
            title: format!("{} waiting on you", count(added.len(), "new task")),
            detail: Some(activity::list_titles(&titles, 3)),
            link: Some(Link::Tasks),
            sources: added.iter().map(|&(_, source)| source).collect(),
            key: None,
        });
    }
    if !closed.is_empty() {
        let titles: Vec<&str> = closed.iter().map(|(title, _)| title.as_str()).collect();
        host.record(Note {
            kind: Kind::Tasks,
            title: format!("{} resolved at the source", count(closed.len(), "task")),
            detail: Some(activity::list_titles(&titles, 3)),
            link: Some(Link::Tasks),
            sources: closed.iter().map(|&(_, source)| source).collect(),
            key: None,
        });
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
