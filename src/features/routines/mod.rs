//! Scheduled tasks: asked for in the chat, kept on the Scheduled page, and
//! run in the background next to the chat, one at a time. A timed task runs
//! with the chat's permissions; a watch is checked read-only, and acts (or
//! just tells the user) once its condition is met. Every outcome goes to
//! Activity.

mod page;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use meow::activity::{Kind, Note};
use meow::claude::{ClaudeEvent, Run};
use meow::conversations::ChatId;
use meow::routines::{self, Change, Check, Routines, Spec, Trigger};

use crate::app::feature::{Feature, Host};
use crate::app::mailbox::Mailbox;
use crate::ui::chat_window::{PageKit, PageView};
use crate::ui::sidebar::PageSpec;
use crate::ui::theme;

use page::RoutinesPage;

const PAGE_ID: &str = "routines";
const CHECK_EVERY: Duration = Duration::from_secs(30);
/// The first look waits a little after launch, so startup stays quick.
const FIRST_CHECK: Duration = Duration::from_secs(25);

enum Msg {
    /// The chat reported a scheduling request in conversation `ChatId`.
    Apply(ChatId, Spec),
    /// The × of this routine on the page.
    Delete(u64),
}

/// What the run in flight is doing for its routine.
enum Stage {
    /// A timed routine doing its job.
    Running,
    /// A watch checking its condition, read-only.
    Checking,
    /// A watch that fired, doing what it was asked to.
    Acting(Check),
}

struct Active {
    run: Run,
    id: u64,
    stage: Stage,
}

pub(crate) struct ScheduledTasks {
    routines: Routines,
    /// Where they are saved; `None` when the file could not be read.
    path: Option<PathBuf>,
    /// Why the saved file could not be read.
    load_error: Option<String>,
    next_check: Instant,
    active: Option<Active>,
    /// The page and the sidebar count need drawing again.
    dirty: bool,
    page: Option<RoutinesPage>,
    inbox: Mailbox<Msg>,
}

impl ScheduledTasks {
    /// The saved routines.
    pub(crate) fn from_env() -> Self {
        let first_check = Instant::now() + FIRST_CHECK;
        let Some(path) = routines::default_path() else {
            return Self::new(Routines::default(), None, None, first_check);
        };
        match Routines::load(&path) {
            Ok(mut list) => {
                list.prune(now_secs());
                Self::new(list, Some(path), None, first_check)
            }
            Err(error) => Self::new(Routines::default(), None, Some(error), first_check),
        }
    }

    fn new(
        routines: Routines,
        path: Option<PathBuf>,
        load_error: Option<String>,
        next_check: Instant,
    ) -> Self {
        Self {
            routines,
            path,
            load_error,
            next_check,
            active: None,
            dirty: true,
            page: None,
            inbox: Mailbox::new(),
        }
    }

    /// The id of the routine being run or checked.
    fn running(&self) -> Option<u64> {
        self.active.as_ref().map(|active| active.id)
    }

    /// Adds, changes or cancels a routine as the chat asked, and says so in `chat`.
    fn apply(&mut self, chat: &ChatId, spec: &Spec, host: &mut dyn Host) {
        let now = now_secs();
        let line = match self.routines.apply(spec, now) {
            Ok(Change::Added(routine)) => format!(
                "Scheduled: {} · {} (see Scheduled)",
                routine.name,
                routine.schedule_label(now)
            ),
            Ok(Change::Updated(routine)) => {
                let id = routine.id;
                let line = format!(
                    "Updated: {} · {} (see Scheduled)",
                    routine.name,
                    routine.schedule_label(now)
                );
                self.stop_run(id);
                line
            }
            Ok(Change::Cancelled(routine)) => {
                let (id, line) = (routine.id, format!("Cancelled: {}", routine.name));
                self.stop_run(id);
                line
            }
            Err(error) => {
                return host.error_in(chat, &format!("Could not schedule that: {error}."));
            }
        };
        host.info_in(chat, &line);
        self.changed();
    }

    fn delete(&mut self, id: u64) {
        // Dropping the run stops it.
        self.stop_run(id);
        if self.routines.remove(id) {
            self.changed();
        }
    }

    /// Drops the run in flight for routine `id`, which changed or went away.
    fn stop_run(&mut self, id: u64) {
        if self.running() == Some(id) {
            self.active = None;
        }
    }

    /// Starts the routine that is due, or collects the run in flight.
    fn poll_runs(&mut self, host: &mut dyn Host) {
        if self.active.is_some() {
            return self.collect(host);
        }
        if Instant::now() < self.next_check {
            return;
        }
        self.next_check = Instant::now() + CHECK_EVERY;
        let now = now_secs();
        let Some(routine) = self.routines.due(now).cloned() else {
            return;
        };
        let Some(env) = host.claude() else {
            return;
        };
        let started = match &routine.trigger {
            Trigger::At { .. } => Run::start(env, &routines::run_prompt(&routine, now), None)
                .map(|run| (run, Stage::Running)),
            Trigger::When { .. } => {
                let Some(prompt) = routines::check_prompt(&routine, now) else {
                    return;
                };
                Run::start_read_only(env, &prompt, &env.model).map(|run| (run, Stage::Checking))
            }
        };
        match started {
            Ok((run, stage)) => {
                self.active = Some(Active {
                    run,
                    id: routine.id,
                    stage,
                });
                self.dirty = true;
            }
            Err(error) => {
                self.routines.postpone(routine.id, now, &error);
                self.changed();
            }
        }
    }

    fn collect(&mut self, host: &mut dyn Host) {
        let outcome = loop {
            let Some(active) = &self.active else {
                return;
            };
            break match active.run.events.try_recv() {
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
                Err(TryRecvError::Disconnected) => Err("the run stopped early".to_string()),
            };
        };
        let Some(Active { id, stage, .. }) = self.active.take() else {
            return;
        };
        match stage {
            Stage::Running => self.ran(id, outcome, host),
            // What the watch found, then what was done about it.
            Stage::Acting(check) => self.ran(
                id,
                outcome.map(|done| format!("{} — {}", check.summary.trim(), done.trim())),
                host,
            ),
            Stage::Checking => self.checked(id, outcome, host),
        }
        self.changed();
    }

    /// A timed routine, or a fired watch's action, finished.
    fn ran(&mut self, id: u64, outcome: Result<String, String>, host: &mut dyn Host) {
        let now = now_secs();
        let Some(name) = self.routines.get(id).map(|r| r.name.clone()) else {
            return;
        };
        let (title, detail) = match &outcome {
            Ok(summary) => (format!("Done: {name}"), summary.trim().to_string()),
            Err(reason) => (format!("Failed: {name}"), reason.clone()),
        };
        let result = match &outcome {
            Ok(summary) => summary.clone(),
            Err(reason) => format!("failed: {reason}"),
        };
        if matches!(
            self.routines.get(id).map(|r| &r.trigger),
            Some(Trigger::At { .. })
        ) {
            self.routines.record_run(id, now, &result);
        }
        host.record(note(title, detail));
    }

    /// A watch was checked: tell the user, or act, when it happened.
    fn checked(&mut self, id: u64, outcome: Result<String, String>, host: &mut dyn Host) {
        let now = now_secs();
        let check = match outcome.and_then(|answer| routines::parse_check(&answer)) {
            Ok(check) => check,
            Err(error) => {
                eprintln!("meow: scheduled watch check failed: {error}");
                self.routines.postpone(id, now, &error);
                return;
            }
        };
        self.routines.record_check(id, now, &check);
        if !check.happened {
            return;
        }
        let Some(routine) = self.routines.get(id).cloned() else {
            return;
        };
        if routine.notify_only {
            let detail = [check.summary.trim(), check.details.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" — ");
            return host.record(note(routine.name.clone(), detail));
        }
        let Some(env) = host.claude() else {
            return;
        };
        match Run::start(env, &routines::act_prompt(&routine, &check, now), None) {
            Ok(run) => {
                self.active = Some(Active {
                    run,
                    id,
                    stage: Stage::Acting(check),
                });
            }
            Err(error) => self.ran(id, Err(error), host),
        }
    }

    /// Saves the routines, and has the page drawn again.
    fn changed(&mut self) {
        self.routines.prune(now_secs());
        if let Some(path) = &self.path
            && let Err(error) = self.routines.save(path)
        {
            eprintln!("meow: {error}");
        }
        self.dirty = true;
    }

    fn render(&mut self, host: &mut dyn Host) {
        if let Some(page) = &self.page {
            page.render(&self.routines, self.running(), now_secs());
        }
        host.set_badge(PAGE_ID, self.routines.active_count());
        self.dirty = false;
    }
}

impl Feature for ScheduledTasks {
    fn poll(&mut self, host: &mut dyn Host) {
        for message in self.inbox.drain() {
            match message {
                Msg::Apply(chat, spec) => self.apply(&chat, &spec, host),
                Msg::Delete(id) => self.delete(id),
            }
        }
        self.poll_runs(host);
        if self.dirty {
            self.render(host);
        }
    }

    fn startup_error(&self) -> Option<String> {
        let error = self.load_error.as_ref()?;
        Some(format!("{error}. Scheduled tasks won't be saved."))
    }

    /// Claude reports "do this later" requests instead of doing them now, and
    /// sees the scheduled tasks there are, to change or restart them.
    fn chat_instruction(&self, now: u64) -> Option<String> {
        Some(routines::instruction(now, &self.routines))
    }

    fn read_reply(&mut self, chat: &ChatId, text: &str, done: bool) -> String {
        let (shown, spec) = routines::extract(text);
        if let (true, Some(spec)) = (done, spec) {
            self.inbox.sender().send(Msg::Apply(chat.clone(), spec));
        }
        shown
    }

    fn pages(&self) -> Vec<PageSpec> {
        vec![PageSpec {
            id: PAGE_ID,
            title: "Scheduled",
            badge: theme::link,
            order: 45,
        }]
    }

    fn build_page(&mut self, _id: &'static str, kit: &PageKit) -> Option<PageView> {
        let inbox = self.inbox.sender();
        let page = RoutinesPage::new(kit.mtm, kit.frame, move |id| inbox.send(Msg::Delete(id)));
        let view = page.view.clone();
        self.page = Some(page);
        self.dirty = true;
        Some(PageView { view, focus: None })
    }

    fn page_shown(&mut self, _id: &'static str) {
        self.dirty = true;
    }

    /// A run in flight, or a request from the chat not applied yet: neither
    /// may be lost to an update's restart.
    fn is_busy(&self) -> bool {
        self.active.is_some() || !self.inbox.is_empty()
    }
}

fn note(title: String, detail: String) -> Note {
    Note {
        kind: Kind::Scheduled,
        title,
        detail: Some(detail),
        link: None,
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
