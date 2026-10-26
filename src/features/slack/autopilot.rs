//! Autopilot on the app side. A Slack message for the user waits until the
//! conversation is quiet, then Claude (no tools) reads it and says whether
//! there is work to prepare. Jobs then run one at a time, in a worktree of the
//! repository when the work is code, with Meow's guard holding every call that
//! cannot be undone. Those wait on the Autopilot page; an approved one is made
//! by resuming the job's session, the guard letting through that call only.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use meow::activity::{Kind, Link, Note};
use meow::autopilot::{self, ActionState, Folder, Job, Jobs, Origin, Stage};
use meow::claude::{ClaudeEvent, Run, STRONG_MODEL};
use meow::guard::{self, Call};
use meow::slack::model::Message;
use meow::source::Source;
use meow::worktree::{self, Tools};

use crate::ui::autopilot_page::{self, AutopilotAction, TAG_STRIDE};

use super::{Slack, SlackSettings, now_secs};
use crate::app::feature::Host;

/// A conversation is triaged once quiet this long, so a request written in
/// several messages is read whole.
const QUIET: Duration = Duration::from_secs(45);
/// At most this many of the conversation's last messages are shown to the triage.
const CONTEXT_MESSAGES: usize = 12;
const POLL_EVERY: Duration = Duration::from_secs(1);
/// The work's model, unless `MEOW_AUTOPILOT_MODEL` says otherwise: it codes.
const MODEL_VAR: &str = "MEOW_AUTOPILOT_MODEL";

/// New messages for the user in a conversation, waiting for it to be quiet.
struct Waiting {
    channel: String,
    /// The last message for the user.
    message: Message,
    /// The whole thread, when the message is a reply in one.
    thread: Vec<Message>,
    /// "Axel", "Axel in #eng".
    from: String,
    due: Instant,
}

enum Step {
    Work,
    /// Making the approved action at this index.
    Approve(usize),
}

struct Active {
    run: Run,
    job: u64,
    step: Step,
    session: Option<String>,
}

pub(super) struct AutopilotState {
    pub(super) jobs: Jobs,
    /// Where jobs are saved; `None` when the file could not be read.
    path: Option<PathBuf>,
    waiting: Vec<Waiting>,
    triage: Option<(Run, Waiting)>,
    preparing: Option<(u64, Receiver<Result<Folder, String>>)>,
    active: Option<Active>,
    next_poll: Instant,
}

impl AutopilotState {
    /// The saved jobs, plus the reason they could not be read.
    pub(super) fn load() -> (Self, Option<String>) {
        let state = |jobs, path| Self {
            jobs,
            path,
            waiting: Vec::new(),
            triage: None,
            preparing: None,
            active: None,
            next_poll: Instant::now(),
        };
        let Some(path) = autopilot::default_path() else {
            return (state(Jobs::default(), None), None);
        };
        match Jobs::load(&path) {
            Ok(mut jobs) => {
                jobs.after_restart();
                remove_folders(jobs.prune(now_secs()));
                (state(jobs, Some(path)), None)
            }
            Err(error) => (
                state(Jobs::default(), None),
                Some(format!("{error}. Autopilot jobs won't be saved.")),
            ),
        }
    }

    /// No jobs and no file, for tests.
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            jobs: Jobs::default(),
            path: None,
            waiting: Vec::new(),
            triage: None,
            preparing: None,
            active: None,
            next_poll: Instant::now(),
        }
    }

    /// The job being worked on or prepared.
    fn busy(&self) -> bool {
        self.active.is_some() || self.preparing.is_some()
    }
}

impl Slack {
    /// A control of the Autopilot page.
    pub(super) fn autopilot_action(&mut self, action: AutopilotAction, host: &mut dyn Host) {
        match action {
            AutopilotAction::Toggle => self.toggle_autopilot(host),
            AutopilotAction::Approve(tag) => self.decide_autopilot(tag, true, host),
            AutopilotAction::Reject(tag) => self.decide_autopilot(tag, false, host),
            AutopilotAction::Dismiss(row) => self.dismiss_autopilot(row, host),
            AutopilotAction::OpenFolder(row) => self.open_autopilot_folder(row),
        }
    }

    /// `message`, from `from`, is for the user: triage its conversation (or
    /// `thread`, when it replies in one) once quiet.
    pub(super) fn consider_for_autopilot(
        &mut self,
        channel: &str,
        message: &Message,
        from: &str,
        thread: &[Message],
    ) {
        if !self.settings.autopilot || self.autopilot.jobs.has_origin(channel, &message.ts) {
            return;
        }
        // A newer message in the same conversation (and thread) replaces the older one.
        self.autopilot
            .waiting
            .retain(|w| w.channel != channel || w.message.thread_ts != message.thread_ts);
        self.autopilot.waiting.push(Waiting {
            channel: channel.to_string(),
            message: message.clone(),
            thread: thread.to_vec(),
            from: from.to_string(),
            due: Instant::now() + QUIET,
        });
    }

    pub(super) fn poll_autopilot(&mut self, host: &mut dyn Host) {
        self.collect_triage(host);
        self.collect_preparing(host);
        self.collect_autopilot_run(host);
        if Instant::now() < self.autopilot.next_poll {
            return;
        }
        self.autopilot.next_poll = Instant::now() + POLL_EVERY;
        self.start_triage(host);
        self.start_next_job(host);
    }

    /// The user's name, as the prompts call them.
    fn me(&self) -> String {
        match self.slack.my_id() {
            Some(_) => self.slack.my_name(),
            None => "the user".to_string(),
        }
    }

    fn start_triage(&mut self, host: &mut dyn Host) {
        if self.autopilot.triage.is_some() {
            return;
        }
        let now = Instant::now();
        let Some(index) = self.autopilot.waiting.iter().position(|w| w.due <= now) else {
            return;
        };
        let waiting = self.autopilot.waiting.remove(index);
        if self.answered_since(&waiting) {
            return;
        }
        let Some(env) = host.claude() else {
            return;
        };
        let prompt = autopilot::triage_prompt(
            &self.me(),
            &waiting.from,
            &self.conversation_text(&waiting),
            autopilot::user_rules().as_deref(),
        );
        match Run::start_offline(env, &prompt) {
            Ok(run) => self.autopilot.triage = Some((run, waiting)),
            Err(error) => eprintln!("meow: autopilot: {error}"),
        }
    }

    /// `thread` of `channel` changed: what waits in it is triaged on the new one.
    pub(super) fn autopilot_thread_changed(&mut self, channel: &str, thread: &[Message]) {
        let Some(parent) = thread.first() else {
            return;
        };
        let waiting = self.autopilot.waiting.iter_mut().filter(|w| {
            w.channel == channel && w.message.thread_ts.as_deref() == Some(parent.ts.as_str())
        });
        for waiting in waiting {
            waiting.thread = thread.to_vec();
        }
    }

    /// The messages around `waiting`'s: its thread, else its conversation.
    fn context<'a>(&'a self, waiting: &'a Waiting) -> &'a [Message] {
        if waiting.thread.is_empty() {
            self.slack.history(&waiting.channel)
        } else {
            &waiting.thread
        }
    }

    /// Whether the user wrote in the conversation after `waiting`'s message.
    fn answered_since(&self, waiting: &Waiting) -> bool {
        let me = self.slack.my_id();
        self.context(waiting)
            .iter()
            .any(|m| m.user.as_deref() == me && me.is_some() && m.ts > waiting.message.ts)
    }

    /// The conversation's last messages, oldest first, as `Name: text`; the
    /// message for the user is always among them.
    fn conversation_text(&self, waiting: &Waiting) -> String {
        let mut messages: Vec<&Message> = self
            .context(waiting)
            .iter()
            .filter(|m| m.ts <= waiting.message.ts)
            .collect();
        if !messages.iter().any(|m| m.ts == waiting.message.ts) {
            messages.push(&waiting.message);
        }
        let start = messages.len().saturating_sub(CONTEXT_MESSAGES);
        messages[start..]
            .iter()
            .map(|m| {
                let name = m
                    .bot_name
                    .clone()
                    .filter(|_| m.user.is_none())
                    .unwrap_or_else(|| self.slack.name_of(m.user.as_deref()));
                format!("{name}: {}", self.slack.text_of(m))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn collect_triage(&mut self, host: &mut dyn Host) {
        let outcome = loop {
            let Some((run, _)) = &self.autopilot.triage else {
                return;
            };
            break match run.events.try_recv() {
                Ok(ClaudeEvent::Done { is_error, result }) => (!is_error).then_some(result),
                Ok(ClaudeEvent::Failed(reason)) => {
                    eprintln!("meow: autopilot triage: {reason}");
                    None
                }
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => None,
            };
        };
        let Some((_, waiting)) = self.autopilot.triage.take() else {
            return;
        };
        let Some(plan) = outcome.as_deref().and_then(autopilot::parse_plan) else {
            return;
        };
        let origin = Origin {
            channel: waiting.channel,
            ts: waiting.message.ts.clone(),
            thread_ts: waiting.message.thread_ts.clone(),
            from: waiting.from,
            text: self.slack.text_of(&waiting.message),
        };
        self.autopilot.jobs.add(plan, origin, now_secs());
        self.autopilot_changed(host);
    }

    /// Starts what comes next: an approved action first, else the oldest queued job.
    fn start_next_job(&mut self, host: &mut dyn Host) {
        if self.autopilot.busy() {
            return;
        }
        if let Some((id, index)) = self.autopilot.jobs.next_approved() {
            return self.start_approval(id, index, host);
        }
        let Some(job) = self.autopilot.jobs.next_queued().cloned() else {
            return;
        };
        if job.needs_folder() {
            self.prepare_folder(&job, host);
        } else {
            self.start_work(job.id, host);
        }
    }

    /// Makes the job's worktree in the background; the work starts once it is there.
    fn prepare_folder(&mut self, job: &Job, host: &mut dyn Host) {
        let (Some(env), Some(dir)) = (host.claude(), autopilot::job_dir(job.id)) else {
            return;
        };
        let tools = Tools {
            path_var: env.path_var.clone(),
            roots: worktree::roots(),
        };
        let (repo, pr, branch) = (job.repo.clone(), job.pr.clone(), job.branch.clone());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let dest = dir.join("work");
            let _ = std::fs::remove_dir_all(&dest);
            let _ =
                tx.send(tools.prepare(repo.as_deref(), pr.as_deref(), branch.as_deref(), &dest));
        });
        if let Some(job) = self.autopilot.jobs.get_mut(job.id) {
            job.stage = Stage::Preparing;
        }
        self.autopilot.preparing = Some((job.id, rx));
        self.autopilot_changed(host);
    }

    fn collect_preparing(&mut self, host: &mut dyn Host) {
        let Some((id, rx)) = &self.autopilot.preparing else {
            return;
        };
        let id = *id;
        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("making the worktree stopped early".into()),
        };
        self.autopilot.preparing = None;
        // Without a worktree the work still runs: a review or an answer needs none.
        if let Some(job) = self.autopilot.jobs.get_mut(id) {
            match outcome {
                Ok(folder) => job.folder = Some(folder),
                Err(error) => job.folder_error = Some(error),
            }
        }
        self.start_work(id, host);
    }

    /// Where job `id` keeps its held calls, and where it runs.
    fn job_paths(&self, job: &Job) -> Option<(PathBuf, PathBuf)> {
        let dir = autopilot::job_dir(job.id)?;
        let cwd = job.cwd(&dir.join("work")).to_path_buf();
        Some((dir.join("held.jsonl"), cwd))
    }

    fn start_work(&mut self, id: u64, host: &mut dyn Host) {
        let Some(job) = self.autopilot.jobs.get(id).cloned() else {
            return;
        };
        let Some(env) = host.claude() else {
            return;
        };
        let Some((held, cwd)) = self.job_paths(&job) else {
            return;
        };
        let _ = std::fs::create_dir_all(&cwd);
        let _ = std::fs::remove_file(&held);
        let model = std::env::var(MODEL_VAR)
            .ok()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| STRONG_MODEL.to_string());
        let prompt = autopilot::work_prompt(&job, &self.me());
        match Run::start_guarded(env, &prompt, None, &model, &cwd, &held, None) {
            Ok(run) => {
                self.autopilot.active = Some(Active {
                    run,
                    job: id,
                    step: Step::Work,
                    session: None,
                });
                if let Some(job) = self.autopilot.jobs.get_mut(id) {
                    job.stage = Stage::Working;
                }
                self.autopilot_changed(host);
            }
            Err(error) => self.job_ended(id, Err(error), host),
        }
    }

    /// Resumes job `id`'s session to make its approved action `index`.
    fn start_approval(&mut self, id: u64, index: usize, host: &mut dyn Host) {
        let Some(job) = self.autopilot.jobs.get(id).cloned() else {
            return;
        };
        let fail = |app: &mut Self, host: &mut dyn Host, reason: &str| {
            app.autopilot
                .jobs
                .set_action(id, index, ActionState::Failed, Some(reason.to_string()));
            app.autopilot_changed(host);
        };
        let (Some(session), Some(action)) = (&job.session, job.actions.get(index)) else {
            return fail(
                self,
                host,
                "The work's Claude session is unknown, so it cannot be resumed.",
            );
        };
        let Some(env) = host.claude() else {
            return;
        };
        let (Some((held, cwd)), Some(dir)) = (self.job_paths(&job), autopilot::job_dir(id)) else {
            return;
        };
        let approved = dir.join("approved.json");
        let written = serde_json::to_string(&action.call)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&approved, json).map_err(|e| e.to_string()));
        if let Err(error) = written {
            return fail(
                self,
                host,
                &format!("Could not write the approval: {error}"),
            );
        }
        let prompt = autopilot::approve_prompt(&action.call, &self.me());
        let model = env.model.clone();
        match Run::start_guarded(
            env,
            &prompt,
            Some(session),
            &model,
            &cwd,
            &held,
            Some(&approved),
        ) {
            Ok(run) => {
                self.autopilot.active = Some(Active {
                    run,
                    job: id,
                    step: Step::Approve(index),
                    session: Some(session.clone()),
                });
                self.autopilot
                    .jobs
                    .set_action(id, index, ActionState::Running, None);
                self.autopilot_changed(host);
            }
            Err(error) => fail(self, host, &error),
        }
    }

    fn collect_autopilot_run(&mut self, host: &mut dyn Host) {
        let outcome = loop {
            let Some(active) = &mut self.autopilot.active else {
                return;
            };
            break match active.run.events.try_recv() {
                Ok(ClaudeEvent::Session(id)) => {
                    active.session = Some(id);
                    continue;
                }
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
        let Some(active) = self.autopilot.active.take() else {
            return;
        };
        if let Some(job) = self.autopilot.jobs.get_mut(active.job) {
            job.session = active.session.or(job.session.take());
        }
        match active.step {
            Step::Work => self.job_ended(active.job, outcome, host),
            Step::Approve(index) => self.approval_ended(active.job, index, outcome, host),
        }
    }

    /// The calls job `id` was refused since they were last read.
    fn take_held(&self, id: u64) -> Vec<Call> {
        let Some(held) = autopilot::job_dir(id).map(|dir| dir.join("held.jsonl")) else {
            return Vec::new();
        };
        let calls = guard::read_held(&held);
        let _ = std::fs::remove_file(&held);
        calls
    }

    /// The work of job `id` ended: its held calls now wait for the user.
    fn job_ended(&mut self, id: u64, outcome: Result<String, String>, host: &mut dyn Host) {
        let held = self.take_held(id);
        self.autopilot.jobs.finish(id, outcome, held, now_secs());
        let Some(job) = self.autopilot.jobs.get(id).cloned() else {
            return;
        };
        let pending = job.pending();
        let (title, detail) = match (job.stage, pending) {
            (Stage::Failed, _) => (
                format!("Autopilot couldn't finish: {}", job.title),
                job.summary.clone(),
            ),
            (_, 0) => (format!("Autopilot did: {}", job.title), job.summary.clone()),
            (_, n) => (
                format!("Autopilot prepared: {}", job.title),
                Some(match n {
                    1 => "1 action waits for your approval.".to_string(),
                    n => format!("{n} actions wait for your approval."),
                }),
            ),
        };
        let sources = if job.folder.is_some() || job.repo.is_some() {
            vec![Source::Slack, Source::GitHub]
        } else {
            vec![Source::Slack]
        };
        host.record(Note {
            kind: Kind::Autopilot,
            title,
            detail,
            link: Some(Link::Autopilot),
            sources,
            key: Some(format!("autopilot:{id}")),
        });
        self.autopilot_changed(host);
    }

    /// The run making approved action `index` of job `id` ended.
    fn approval_ended(
        &mut self,
        id: u64,
        index: usize,
        outcome: Result<String, String>,
        host: &mut dyn Host,
    ) {
        let approved = autopilot::job_dir(id).map(|dir| dir.join("approved.json"));
        // The guard removes the approval once the call went through.
        let unused = approved.as_ref().is_some_and(|path| path.exists());
        if let Some(path) = &approved {
            let _ = std::fs::remove_file(path);
        }
        let (state, result) = match (outcome, unused) {
            (Ok(result), false) => (ActionState::Done, result),
            (Ok(result), true) => (
                ActionState::Failed,
                format!("Claude did not make the call. {result}"),
            ),
            (Err(reason), _) => (ActionState::Failed, reason),
        };
        self.autopilot
            .jobs
            .set_action(id, index, state, Some(result));
        let held = self.take_held(id);
        self.autopilot.jobs.hold(id, held);
        self.autopilot_changed(host);
    }

    pub(super) fn toggle_autopilot(&mut self, host: &mut dyn Host) {
        self.settings = SlackSettings {
            autopilot: !self.settings.autopilot,
            ..self.settings.clone()
        };
        if !self.settings.autopilot {
            self.autopilot.waiting.clear();
        }
        if let Err(error) = host.settings().set(&self.settings) {
            eprintln!("meow: {error}");
        }
        self.render_autopilot(host);
    }

    /// Approves (or rejects) the action a button's `tag` points at.
    pub(super) fn decide_autopilot(&mut self, tag: usize, approve: bool, host: &mut dyn Host) {
        let (row, index) = (tag / TAG_STRIDE, tag % TAG_STRIDE);
        let Some(id) = self.autopilot_page().id_at(row) else {
            return;
        };
        let pending = self
            .autopilot
            .jobs
            .get(id)
            .and_then(|job| job.actions.get(index))
            .is_some_and(|action| action.state == ActionState::Pending);
        if !pending {
            return;
        }
        let state = if approve {
            ActionState::Approved
        } else {
            ActionState::Rejected
        };
        self.autopilot.jobs.set_action(id, index, state, None);
        self.autopilot_changed(host);
    }

    /// Removes the job in `row`, stopping its run and deleting its worktree.
    pub(super) fn dismiss_autopilot(&mut self, row: usize, host: &mut dyn Host) {
        let Some(id) = self.autopilot_page().id_at(row) else {
            return;
        };
        // Dropping the run stops it.
        if self.autopilot.active.as_ref().is_some_and(|a| a.job == id) {
            self.autopilot.active = None;
        }
        if self
            .autopilot
            .preparing
            .as_ref()
            .is_some_and(|(job, _)| *job == id)
        {
            self.autopilot.preparing = None;
        }
        if let Some(job) = self.autopilot.jobs.remove(id) {
            remove_folders(vec![job]);
        }
        self.autopilot_changed(host);
    }

    pub(super) fn open_autopilot_folder(&self, row: usize) {
        let folder = self
            .autopilot_page()
            .id_at(row)
            .and_then(|id| self.autopilot.jobs.get(id))
            .and_then(|job| job.folder.as_ref());
        if let Some(folder) = folder {
            let _ = std::process::Command::new("open").arg(&folder.path).spawn();
        }
    }

    fn autopilot_changed(&mut self, host: &mut dyn Host) {
        remove_folders(self.autopilot.jobs.prune(now_secs()));
        if let Some(path) = &self.autopilot.path
            && let Err(error) = self.autopilot.jobs.save(path)
        {
            eprintln!("meow: {error}");
        }
        self.render_autopilot(host);
    }

    /// Redraws the Autopilot page, and the count of jobs waiting in the sidebar.
    pub(super) fn render_autopilot(&self, host: &mut dyn Host) {
        let jobs = &self.autopilot.jobs;
        self.autopilot_page()
            .render(jobs, self.settings.autopilot, now_secs());
        host.set_badge(autopilot_page::PAGE_ID, jobs.waiting_count());
    }
}

/// Deletes the worktrees and files of `jobs`, in the background.
fn remove_folders(jobs: Vec<Job>) {
    if jobs.is_empty() {
        return;
    }
    thread::spawn(move || {
        let tools = Tools {
            path_var: std::env::var("PATH").unwrap_or_default(),
            roots: Vec::new(),
        };
        for job in jobs {
            if let Some(folder) = &job.folder
                && let Err(error) = tools.remove(folder)
            {
                eprintln!("meow: autopilot: {error}");
            }
            if let Some(dir) = autopilot::job_dir(job.id) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    });
}
