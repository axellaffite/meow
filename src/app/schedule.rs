//! Daily quick actions (Today's planning at 7:00): each runs on its own once a
//! day, at its time or as soon as Meow is running after it (Mac asleep, logged
//! in late…). The run happens in the background; the cat shows `!` when done.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use meow::schedule;

use super::{App, now_secs};

const CHECK_EVERY: Duration = Duration::from_secs(60);
/// The first check waits a little after launch, so startup stays quick.
const FIRST_CHECK: Duration = Duration::from_secs(20);

pub(super) struct ScheduleState {
    next_check: Instant,
    /// When each daily action was last started by the schedule, by title.
    tried: HashMap<String, u64>,
}

impl ScheduleState {
    pub(super) fn new() -> Self {
        Self {
            next_check: Instant::now() + FIRST_CHECK,
            tried: HashMap::new(),
        }
    }
}

impl App {
    /// Starts a daily action that is due. It waits while a request runs or the
    /// bubble is open, so it never gets in the way.
    pub(super) fn poll_schedule(&mut self) {
        if Instant::now() < self.schedule.next_check {
            return;
        }
        self.schedule.next_check = Instant::now() + CHECK_EVERY;
        let ready = matches!(self.claude, Some(Ok(_)));
        if !ready || !self.runs.is_empty() || self.chat.is_visible() {
            return;
        }
        let now = now_secs();
        let due = self.actions.iter().position(|action| {
            let Some(slot) = action.daily_at.and_then(|at| at.today(now)) else {
                return false;
            };
            let last_ran = self.action_results.get(&action.title).map(|r| r.ran_at);
            let last_try = self.schedule.tried.get(&action.title).copied();
            schedule::due(slot, now, last_ran, last_try)
        });
        if let Some(index) = due {
            self.schedule
                .tried
                .insert(self.actions[index].title.clone(), now);
            self.run_action(index);
        }
    }
}
