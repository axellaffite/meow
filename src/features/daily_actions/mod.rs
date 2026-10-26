//! Daily quick actions (Today's planning at 7:00): each runs on its own once a
//! day, at its time or as soon as Meow is running after it (Mac asleep, logged
//! in late…). The run happens in the background; the cat shows `!` when done.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use meow::schedule;

use crate::app::feature::{Feature, Host};

const CHECK_EVERY: Duration = Duration::from_secs(60);
/// The first check waits a little after launch, so startup stays quick.
const FIRST_CHECK: Duration = Duration::from_secs(20);

pub(crate) struct DailyActions {
    next_check: Instant,
    /// When each daily action was last started by the schedule, by title.
    tried: HashMap<String, u64>,
}

impl DailyActions {
    pub(crate) fn new() -> Self {
        Self {
            next_check: Instant::now() + FIRST_CHECK,
            tried: HashMap::new(),
        }
    }
}

impl Feature for DailyActions {
    /// Starts a daily action that is due. It waits while a request runs or
    /// the bubble is open, so it never gets in the way.
    fn poll(&mut self, host: &mut dyn Host) {
        if Instant::now() < self.next_check {
            return;
        }
        self.next_check = Instant::now() + CHECK_EVERY;
        if host.claude().is_none() || host.chat_in_use() {
            return;
        }
        let now = now_secs();
        let due = host.quick_actions().iter().position(|action| {
            let Some(slot) = action.daily_at.and_then(|at| at.today(now)) else {
                return false;
            };
            let last_ran = host.last_result(&action.title).map(|(ran_at, _)| ran_at);
            let last_try = self.tried.get(&action.title).copied();
            schedule::due(slot, now, last_ran, last_try)
        });
        if let Some(index) = due {
            let title = host.quick_actions()[index].title.clone();
            self.tried.insert(title, now);
            host.run_quick_action(index);
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
