//! Sorting the tasks the check, quick actions and plans add, before they
//! show: one that repeats a request already listed is merged into it, and a
//! message to answer waits out a grace period, as I have often answered it
//! before I look at my tasks.

use std::collections::HashSet;

use meow::tasks::{self, Task, TaskList};
use serde::{Deserialize, Serialize};

/// The grace periods Settings offers, in minutes; 0 turns it off.
pub(super) const GRACE_MINUTES: [u64; 5] = [0, 10, 20, 30, 60];

/// Settings › Tasks, saved with the other settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TriageSettings {
    /// How long a Slack message or an email to answer stays out of sight
    /// after it arrived; 0 shows it at once.
    pub(super) task_grace_minutes: u64,
    /// Whether a task about a request already listed is merged into it.
    pub(super) merge_repeat_tasks: bool,
}

impl Default for TriageSettings {
    fn default() -> Self {
        Self {
            task_grace_minutes: 30,
            merge_repeat_tasks: true,
        }
    }
}

/// What one pass changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Sorted {
    pub(super) merged: usize,
    pub(super) held: usize,
}

impl Sorted {
    pub(super) fn changed(self) -> bool {
        self.merged + self.held > 0
    }
}

pub(super) struct Triage {
    pub(super) settings: TriageSettings,
    /// Open tasks already sorted; `None` until the first pass, which only
    /// learns the list, so the tasks shown at launch stay as they are.
    seen: Option<HashSet<String>>,
}

impl Triage {
    pub(super) fn new(settings: TriageSettings) -> Self {
        Self {
            settings,
            seen: None,
        }
    }

    /// Sorts the open tasks not seen before, oldest first.
    pub(super) fn sort(&mut self, list: &mut TaskList, now: u64) -> Sorted {
        let Some(seen) = &mut self.seen else {
            self.seen = Some(list.open().into_iter().map(|t| t.id.clone()).collect());
            return Sorted::default();
        };
        // Runs every tick: only the new tasks are copied.
        let new: Vec<Task> = list
            .open()
            .into_iter()
            .filter(|t| !seen.contains(&t.id))
            .cloned()
            .collect();
        let mut sorted = Sorted::default();
        for task in new {
            seen.insert(task.id.clone());
            if self.settings.merge_repeat_tasks && list.repeat_of(&task.id).is_some() {
                if list.absorb(&task.id, now) {
                    sorted.merged += 1;
                }
                continue;
            }
            let until = grace_end(&task, self.settings.task_grace_minutes);
            if until.is_some_and(|until| until > now && list.hold(&task.id, until)) {
                sorted.held += 1;
            }
        }
        sorted
    }
}

/// When the grace period of `task` ends: only messages to answer (Slack,
/// Gmail) have one, counted from when the Slack message was posted, or else
/// from when the task was found.
pub(super) fn grace_end(task: &Task, minutes: u64) -> Option<u64> {
    let message = ["slack", "gmail"].contains(&task.source.to_lowercase().as_str());
    if minutes == 0 || !message {
        return None;
    }
    let from = tasks::message_time(task).unwrap_or(task.created_at);
    Some(from + minutes * 60)
}

/// "Off", "20 min", "1 h".
pub(super) fn grace_label(minutes: u64) -> String {
    match minutes {
        0 => "Off".to_string(),
        m if m % 60 == 0 => format!("{} h", m / 60),
        m => format!("{m} min"),
    }
}
