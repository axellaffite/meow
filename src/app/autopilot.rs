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
use meow::settings::Settings;
use meow::slack::model::Message;
use meow::source::Source;
use meow::worktree::{self, Tools};

use crate::ui::autopilot_page::TAG_STRIDE;
use crate::ui::sidebar::Page;

use super::{App, now_secs};

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

    /// The job being worked on or prepared.
    fn busy(&self) -> bool {
        self.active.is_some() || self.preparing.is_some()
    }
}

impl App {
    /// `message`, from `from`, is for the user: triage its conversation once quiet.
    pub(super) fn consider_for_autopilot(&mut self, channel: &str, message: &Message, from: &str) {
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
            from: from.to_string(),
            due: Instant::now() + QUIET,
        });
    }

    pub(super) fn poll_autopilot(&mut self) {
        self.collect_triage();
        self.collect_preparing();
        self.collect_autopilot_run();
        if Instant::now() < self.autopilot.next_poll {
            return;
        }
        self.autopilot.next_poll = Instant::now() + POLL_EVERY;
        self.start_triage();
        self.start_next_job();
    }

    /// The user's name, as the prompts call them.
    fn me(&self) -> String {
        match self.slack.my_id() {
            Some(_) => self.slack.my_name(),
            None => "the user".to_string(),
        }
    }

    fn start_triage(&mut self) {
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
        let Some(Ok(env)) = &self.claude else {
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

    /// Whether the user wrote in the conversation after `waiting`'s message.
    fn answered_since(&self, waiting: &Waiting) -> bool {
        let me = self.slack.my_id();
        self.slack
            .history(&waiting.channel)
            .iter()
            .any(|m| m.user.as_deref() == me && me.is_some() && m.ts > waiting.message.ts)
    }

    /// The conversation's last messages, oldest first, as `Name: text`; the
    /// message for the user is always among them.
    fn conversation_text(&self, waiting: &Waiting) -> String {
        let history = self.slack.history(&waiting.channel);
        let mut messages: Vec<&Message> = history
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

    fn collect_triage(&mut self) {
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
        self.autopilot_changed();
    }

    /// Starts what comes next: an approved action first, else the oldest queued job.
    fn start_next_job(&mut self) {
        if self.autopilot.busy() {
            return;
        }
        if let Some((id, index)) = self.autopilot.jobs.next_approved() {
            return self.start_approval(id, index);
        }
        let Some(job) = self.autopilot.jobs.next_queued().cloned() else {
            return;
        };
        let wants_code = job.repo.is_some() || job.pr.is_some();
        if wants_code && job.folder.is_none() {
            self.prepare_folder(&job);
        } else {
            self.start_work(job.id);
        }
    }

    /// Makes the job's worktree in the background; the work starts once it is there.
    fn prepare_folder(&mut self, job: &Job) {
        let (Some(Ok(env)), Some(dir)) = (&self.claude, autopilot::job_dir(job.id)) else {
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
        self.autopilot_changed();
    }

    fn collect_preparing(&mut self) {
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
        match outcome {
            Ok(folder) => {
                if let Some(job) = self.autopilot.jobs.get_mut(id) {
                    job.folder = Some(folder);
                }
                self.start_work(id);
            }
            Err(error) => self.job_ended(id, Err(error)),
        }
    }

    /// Where job `id` keeps its held calls, and where it runs.
    fn job_paths(&self, job: &Job) -> Option<(PathBuf, PathBuf)> {
        let dir = autopilot::job_dir(job.id)?;
        let cwd = job.cwd(&dir.join("work")).to_path_buf();
        Some((dir.join("held.jsonl"), cwd))
    }

    fn start_work(&mut self, id: u64) {
        let Some(job) = self.autopilot.jobs.get(id).cloned() else {
            return;
        };
        let Some(Ok(env)) = &self.claude else {
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
                self.autopilot_changed();
            }
            Err(error) => self.job_ended(id, Err(error)),
        }
    }

    /// Resumes job `id`'s session to make its approved action `index`.
    fn start_approval(&mut self, id: u64, index: usize) {
        let Some(job) = self.autopilot.jobs.get(id).cloned() else {
            return;
        };
        let fail = |app: &mut Self, reason: &str| {
            app.autopilot
                .jobs
                .set_action(id, index, ActionState::Failed, Some(reason.to_string()));
            app.autopilot_changed();
        };
        let (Some(session), Some(action)) = (&job.session, job.actions.get(index)) else {
            return fail(
                self,
                "The work's Claude session is unknown, so it cannot be resumed.",
            );
        };
        let Some(Ok(env)) = &self.claude else {
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
            return fail(self, &format!("Could not write the approval: {error}"));
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
                self.autopilot_changed();
            }
            Err(error) => fail(self, &error),
        }
    }

    fn collect_autopilot_run(&mut self) {
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
            Step::Work => self.job_ended(active.job, outcome),
            Step::Approve(index) => self.approval_ended(active.job, index, outcome),
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
    fn job_ended(&mut self, id: u64, outcome: Result<String, String>) {
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
        self.notify(Note {
            kind: Kind::Autopilot,
            title,
            detail,
            link: Some(Link::Autopilot),
            sources,
            key: Some(format!("autopilot:{id}")),
        });
        self.autopilot_changed();
    }

    /// The run making approved action `index` of job `id` ended.
    fn approval_ended(&mut self, id: u64, index: usize, outcome: Result<String, String>) {
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
        self.autopilot_changed();
    }

    pub(super) fn show_autopilot(&mut self) {
        if !self.chat.is_visible() {
            self.open_window();
        }
        self.render_autopilot();
        self.chat.show_page(Page::Autopilot);
    }

    pub(super) fn toggle_autopilot(&mut self) {
        self.settings = Settings {
            autopilot: !self.settings.autopilot,
            ..self.settings.clone()
        };
        if !self.settings.autopilot {
            self.autopilot.waiting.clear();
        }
        if let Some(path) = &self.settings_path
            && let Err(error) = self.settings.save(path)
        {
            eprintln!("meow: {error}");
        }
        self.render_autopilot();
    }

    /// Approves (or rejects) the action a button's `tag` points at.
    pub(super) fn decide_autopilot(&mut self, tag: usize, approve: bool) {
        let (row, index) = (tag / TAG_STRIDE, tag % TAG_STRIDE);
        let Some(id) = self.chat.autopilot().id_at(row) else {
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
        self.autopilot_changed();
    }

    /// Removes the job in `row`, stopping its run and deleting its worktree.
    pub(super) fn dismiss_autopilot(&mut self, row: usize) {
        let Some(id) = self.chat.autopilot().id_at(row) else {
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
        self.autopilot_changed();
    }

    pub(super) fn open_autopilot_folder(&self, row: usize) {
        let folder = self
            .chat
            .autopilot()
            .id_at(row)
            .and_then(|id| self.autopilot.jobs.get(id))
            .and_then(|job| job.folder.as_ref());
        if let Some(folder) = folder {
            let _ = std::process::Command::new("open").arg(&folder.path).spawn();
        }
    }

    fn autopilot_changed(&mut self) {
        remove_folders(self.autopilot.jobs.prune(now_secs()));
        if let Some(path) = &self.autopilot.path
            && let Err(error) = self.autopilot.jobs.save(path)
        {
            eprintln!("meow: {error}");
        }
        self.render_autopilot();
    }

    pub(super) fn render_autopilot(&self) {
        self.chat
            .render_autopilot(&self.autopilot.jobs, self.settings.autopilot, now_secs());
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
