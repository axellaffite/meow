//! The background check of tasks: every few minutes, a quiet Claude session
//! removes resolved tasks and adds new ones. It runs next to the chat, never
//! blocks it, and reports in the Tasks header and in Activity. It also sorts
//! every new task before it shows (see `triage`), with its settings in
//! Settings › Tasks.

#[cfg(test)]
mod tests;
pub(crate) mod triage;

use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use meow::activity::{self, Kind, Link, Note};
use meow::claude::{ClaudeEvent, Run};
use meow::clock;
use meow::source::Source;
use meow::sync;

use crate::app::feature::{Feature, Host};
use crate::app::mailbox::Mailbox;
use crate::ui::settings_page::{SettingEntry, SettingsSection};

use triage::{GRACE_MINUTES, Triage, TriageSettings};

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
    triage: Triage,
    /// Why the saved settings could not be read.
    load_error: Option<String>,
    inbox: Mailbox<Msg>,
}

#[derive(Clone)]
enum Msg {
    Grace(u64),
    MergeRepeats(bool),
}

impl BackgroundCheck {
    /// The saved sorting settings (the defaults when they cannot be read),
    /// and `MEOW_SYNC_MINUTES` (default 5, 0 turns checks off).
    pub(crate) fn from_settings(saved: Result<TriageSettings, String>) -> Self {
        let minutes = std::env::var("MEOW_SYNC_MINUTES")
            .ok()
            .and_then(|m| m.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_MINUTES);
        let interval = (minutes > 0).then(|| Duration::from_secs(minutes * 60));
        let (settings, load_error) = match saved {
            Ok(settings) => (settings, None),
            Err(error) => (TriageSettings::default(), Some(error)),
        };
        Self {
            load_error,
            ..Self::with_settings(interval, settings)
        }
    }

    #[cfg(test)]
    fn new(interval: Option<Duration>) -> Self {
        Self::with_settings(interval, TriageSettings::default())
    }

    fn with_settings(interval: Option<Duration>, settings: TriageSettings) -> Self {
        Self {
            interval,
            next_at: Instant::now() + FIRST_DELAY,
            run: None,
            started_at: 0,
            last_ok: None,
            last_failed: false,
            status_shown_at: Instant::now(),
            triage: Triage::new(settings),
            load_error: None,
            inbox: Mailbox::new(),
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
                // Held tasks were never shown: they go as quietly as they came.
                let closed_tasks: Vec<(String, Source)> = report
                    .done
                    .iter()
                    .filter_map(|id| tasks.get(id))
                    .filter(|task| !task.is_done() && !task.is_held())
                    .map(|task| (task.title.clone(), Source::parse(&task.source)))
                    .collect();
                let new_found = sync::fresh(&report, tasks);
                let closed = tasks.close_resolved(&report.done, now);
                let added = tasks.merge(&new_found, now);
                let sorted = self.triage.sort(tasks, now);
                // Only what shows now is news; held tasks are told when they show.
                let new_tasks: Vec<(String, Source)> = new_found
                    .iter()
                    .filter_map(|found| tasks.find(found))
                    .filter(|task| !task.is_done() && !task.is_held())
                    .map(|task| (task.title.clone(), Source::parse(&task.source)))
                    .collect();
                if closed + added > 0 || sorted.changed() {
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
        let held = host.tasks().held().len();
        let status = match (status, held) {
            (status, 0) => status,
            (Some(status), held) => Some(format!("{status} · {held} on hold")),
            (None, held) => Some(format!("{held} on hold")),
        };
        host.set_tasks_status(status.as_deref());
    }

    /// Sorts the tasks added since the last pass (by quick actions and
    /// plans, too), and shows the held ones whose grace period is over.
    fn sort_tasks(&mut self, host: &mut dyn Host) {
        let now = now_secs();
        let tasks = host.tasks();
        let sorted = self.triage.sort(tasks, now);
        let released = tasks.release_held(now);
        if !sorted.changed() && released.is_empty() {
            return;
        }
        host.tasks_changed();
        let shown: Vec<(String, Source)> = released
            .iter()
            .map(|task| (task.title.clone(), Source::parse(&task.source)))
            .collect();
        announce(host, &shown, &[]);
        self.show_status(host);
    }

    fn handle(&mut self, message: Msg, host: &mut dyn Host) {
        let settings = &mut self.triage.settings;
        match message {
            Msg::Grace(minutes) => settings.task_grace_minutes = minutes,
            Msg::MergeRepeats(on) => settings.merge_repeat_tasks = on,
        }
        if settings.task_grace_minutes == 0 {
            let released = host.tasks().release_held(u64::MAX);
            if !released.is_empty() {
                host.tasks_changed();
                self.show_status(host);
            }
        }
        host.settings_changed();
        if let Err(error) = host.settings().set(&self.triage.settings) {
            eprintln!("meow: {error}");
        }
    }

    fn task_entries(&self) -> Vec<SettingEntry> {
        let settings = &self.triage.settings;
        let grace = self.inbox.sender();
        let merge = self.inbox.sender();
        let merging = settings.merge_repeat_tasks;
        vec![
            SettingEntry::Choice {
                title: "Grace period for messages to answer".to_string(),
                options: GRACE_MINUTES
                    .iter()
                    .map(|&m| triage::grace_label(m))
                    .collect(),
                selected: GRACE_MINUTES
                    .iter()
                    .position(|&m| m == settings.task_grace_minutes),
                pick: Box::new(move |index| {
                    if let Some(&minutes) = GRACE_MINUTES.get(index) {
                        grace.send(Msg::Grace(minutes));
                    }
                }),
            },
            SettingEntry::Note(
                "A Slack message or an email to answer joins the list only this long after it \
                 arrived, if you have not answered it by then: you often have."
                    .to_string(),
            ),
            SettingEntry::Toggle {
                title: "Merge repeats of the same request".to_string(),
                on: merging,
                run: Box::new(move || merge.send(Msg::MergeRepeats(!merging))),
            },
            SettingEntry::Note(
                "A pull request in GitHub and in Linear, a Linear issue and the Slack messages \
                 about it, or several messages of one DM make one task."
                    .to_string(),
            ),
        ]
    }
}

impl Feature for BackgroundCheck {
    /// Starts a check when one is due, and collects the one in flight.
    fn poll(&mut self, host: &mut dyn Host) {
        for message in self.inbox.drain() {
            self.handle(message, host);
        }
        self.sort_tasks(host);
        if self.run.is_some() {
            self.collect(host);
        } else if self.is_due() && host.claude().is_some() {
            self.start(host);
        }
        if self.status_shown_at.elapsed() >= STATUS_REFRESH {
            self.show_status(host);
        }
    }

    fn startup_error(&self) -> Option<String> {
        self.load_error.clone()
    }

    fn settings(&self) -> Vec<SettingsSection> {
        vec![SettingsSection {
            title: "Tasks",
            entries: self.task_entries(),
        }]
    }

    fn is_busy(&self) -> bool {
        self.run.is_some()
    }
}

/// One Activity item for the tasks that showed, with a banner, and one,
/// quiet, for those the check closed, each with the sources they come from.
fn announce(host: &mut dyn Host, added: &[(String, Source)], closed: &[(String, Source)]) {
    let count = |n: usize, what: &str| match n {
        1 => format!("1 {what}"),
        n => format!("{n} {what}s"),
    };
    if !added.is_empty() {
        let titles: Vec<&str> = added.iter().map(|(title, _)| title.as_str()).collect();
        host.record(Note {
            kind: Kind::Tasks,
            title: format!("{} waiting on you", count(added.len(), "new task")),
            detail: Some(activity::list_titles(&titles, 3)),
            link: Some(Link::Tasks),
            sources: added.iter().map(|(_, source)| *source).collect(),
            key: None,
        });
    }
    if !closed.is_empty() {
        let titles: Vec<&str> = closed.iter().map(|(title, _)| title.as_str()).collect();
        host.record_quietly(Note {
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
