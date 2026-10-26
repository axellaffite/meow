//! The task list: things to do, filled by quick actions and by hand.
//!
//! Claude reports tasks in a fenced `meow-tasks` JSON block at the end of an
//! answer; the app strips it from the chat and merges it into the list.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Info string of the fenced block Claude uses to report tasks.
pub const TASKS_FENCE: &str = "meow-tasks";
/// Done tasks stay in the completed list this long after being checked.
pub const DONE_TTL_SECS: u64 = 30 * 24 * 60 * 60;
const MAX_FOUND: usize = 50;
const MAX_TITLE_CHARS: usize = 200;
const MANUAL_SOURCE: &str = "Me";
/// Slack messages that need no answer, so "ok" or "super merci" never becomes a task, and
/// neither does an update that only tells me something is decided or done.
pub const SLACK_NO_REPLY_RULE: &str = "On Slack, a message that only acknowledges, thanks, \
     agrees or wraps up (\"ok\", \"thanks\", \"super merci\", \"parfait\", a thumbs up), in any \
     language, needs no answer and is never a task. So does a message that only informs me of \
     a decision or a result (\"je te confirme\", \"c'est fait\") with no question and no request \
     to me.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub source: String,
    #[serde(default)]
    pub url: Option<String>,
    pub created_at: u64,
    #[serde(default)]
    pub done_at: Option<u64>,
    /// Claude session of this task's own conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Ticked off by the background check rather than by hand.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_done: bool,
}

impl Task {
    pub fn is_done(&self) -> bool {
        self.done_at.is_some()
    }
}

/// A task as Claude reports it, before it joins the list.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct FoundTask {
    pub title: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// The tasks of one source, as shown under its header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskGroup<'a> {
    pub source: &'a str,
    pub tasks: Vec<&'a Task>,
}

impl TaskGroup<'_> {
    pub fn open_count(&self) -> usize {
        self.tasks.iter().filter(|t| !t.is_done()).count()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    tasks: Vec<Task>,
    /// Tasks the background check found resolved: removed from the list, but kept
    /// so the check never reports them again. Forgotten after [`DONE_TTL_SECS`].
    #[serde(default)]
    dismissed: Vec<Task>,
}

impl TaskList {
    /// Open tasks in the order they arrived, then done ones, most recent first.
    pub fn display(&self) -> Vec<&Task> {
        let mut done: Vec<&Task> = self.tasks.iter().filter(|t| t.is_done()).collect();
        done.sort_by_key(|t| std::cmp::Reverse(t.done_at));
        self.tasks
            .iter()
            .filter(|t| !t.is_done())
            .chain(done)
            .collect()
    }

    /// Open tasks grouped by source. Groups keep the order their source first
    /// appeared in, so ticking a task never reshuffles them; done tasks are
    /// left out (see [`TaskList::completed`]).
    pub fn groups(&self) -> Vec<TaskGroup<'_>> {
        let mut groups: Vec<TaskGroup> = Vec::new();
        for task in &self.tasks {
            if !groups.iter().any(|g| g.source == task.source) {
                groups.push(TaskGroup {
                    source: &task.source,
                    tasks: Vec::new(),
                });
            }
        }
        for task in self.tasks.iter().filter(|t| !t.is_done()) {
            if let Some(group) = groups.iter_mut().find(|g| g.source == task.source) {
                group.tasks.push(task);
            }
        }
        groups.retain(|g| !g.tasks.is_empty());
        groups
    }

    /// Done tasks, most recently done first.
    pub fn completed(&self) -> Vec<&Task> {
        self.display().into_iter().filter(|t| t.is_done()).collect()
    }

    pub fn open_count(&self) -> usize {
        self.tasks.iter().filter(|t| !t.is_done()).count()
    }

    pub fn add_manual(&mut self, title: &str, now: u64) -> bool {
        let Some(title) = clean_title(title) else {
            return false;
        };
        let id = (self.tasks.len()..)
            .map(|n| format!("manual:{now}:{n}"))
            .find(|id| self.get(id).is_none())
            .expect("an unused id exists");
        self.tasks.push(Task {
            id,
            title,
            source: MANUAL_SOURCE.to_string(),
            url: None,
            created_at: now,
            done_at: None,
            session: None,
            auto_done: false,
        });
        true
    }

    /// Adds new tasks; ones already listed keep their done state. Returns how many were added.
    pub fn merge(&mut self, found: &[FoundTask], now: u64) -> usize {
        let mut added = 0;
        for item in found.iter().take(MAX_FOUND) {
            let Some(title) = clean_title(&item.title) else {
                continue;
            };
            let source = clean_source(item.source.as_deref());
            let url = clean_url(item.url.as_deref());
            let id = task_key(&source, &title, url);
            let probe = FoundTask {
                title: title.clone(),
                source: Some(source.clone()),
                url: url.map(str::to_string),
            };
            // A task already listed, even done, keeps its state; a dismissed one stays gone.
            match self.position(&probe) {
                Some(index) => self.tasks[index].title = title,
                None if self.find(&probe).is_some() => {}
                None => {
                    self.tasks.push(Task {
                        id,
                        title,
                        source,
                        url: url.map(str::to_string),
                        created_at: now,
                        done_at: None,
                        session: None,
                        auto_done: false,
                    });
                    added += 1;
                }
            }
        }
        added
    }

    pub fn set_done(&mut self, id: &str, done: bool, now: u64) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.done_at = done.then_some(now);
            task.auto_done = false;
        }
    }

    pub fn open(&self) -> Vec<&Task> {
        self.tasks.iter().filter(|t| !t.is_done()).collect()
    }

    /// Removes open tasks the background check found resolved; unknown or
    /// already done ids are ignored. Returns how many were removed.
    pub fn close_resolved(&mut self, ids: &[String], now: u64) -> usize {
        let (resolved, kept): (Vec<Task>, Vec<Task>) = std::mem::take(&mut self.tasks)
            .into_iter()
            .partition(|t| !t.is_done() && ids.contains(&t.id));
        self.tasks = kept;
        let closed = resolved.len();
        self.dismissed.extend(resolved.into_iter().map(|mut task| {
            task.done_at = Some(now);
            task.auto_done = true;
            task
        }));
        closed
    }

    pub fn set_session(&mut self, id: &str, session: Option<&str>) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.session = session.map(str::to_string);
        }
    }

    /// The listed task `found` is about: same id, same link (ignoring its
    /// query, so a Slack thread link matches its message link), or same
    /// source and title.
    pub fn find(&self, found: &FoundTask) -> Option<&Task> {
        self.position(found)
            .map(|index| &self.tasks[index])
            .or_else(|| position_in(&self.dismissed, found).map(|index| &self.dismissed[index]))
    }

    fn position(&self, found: &FoundTask) -> Option<usize> {
        position_in(&self.tasks, found)
    }

    /// Every task done within [`DONE_TTL_SECS`], dismissed ones included, most
    /// recent first, so a check does not report them again.
    pub fn recently_done(&self) -> Vec<&Task> {
        let mut done = self.completed();
        done.extend(self.dismissed.iter());
        done.sort_by_key(|t| std::cmp::Reverse(t.done_at));
        done
    }

    pub fn get(&self, id: &str) -> Option<&Task> {
        self.tasks
            .iter()
            .chain(&self.dismissed)
            .find(|t| t.id == id)
    }

    /// Forgets tasks that were done more than [`DONE_TTL_SECS`] ago.
    pub fn prune(&mut self, now: u64) {
        let keep = |t: &Task| {
            t.done_at
                .is_none_or(|done| now.saturating_sub(done) < DONE_TTL_SECS)
        };
        self.tasks.retain(keep);
        self.dismissed.retain(keep);
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("Could not read tasks from {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Could not read {}: {e}", path.display())),
        }
    }

    /// Writes atomically, so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let fail =
            |e: &dyn std::fmt::Display| format!("Could not save tasks to {}: {e}", path.display());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| fail(&e))?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, json).map_err(|e| fail(&e))?;
        std::fs::rename(&temp, path).map_err(|e| fail(&e))
    }
}

/// `~/Library/Application Support/Meow/tasks.json`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/Meow/tasks.json"))
}

/// The id `found` has, or would have, in the list; `None` if it has no title.
/// Index in `tasks` of the task `found` is about, see [`TaskList::find`].
fn position_in(tasks: &[Task], found: &FoundTask) -> Option<usize> {
    let id = id_for(found)?;
    let link = clean_url(found.url.as_deref()).map(link_key);
    let title = clean_title(&found.title)?.to_lowercase();
    let source = clean_source(found.source.as_deref()).to_lowercase();
    tasks.iter().position(|t| t.id == id).or_else(|| {
        tasks.iter().position(|t| match (&link, t.url.as_deref()) {
            (Some(link), Some(url)) => *link == link_key(url),
            _ => t.source.to_lowercase() == source && t.title.to_lowercase() == title,
        })
    })
}

pub fn id_for(found: &FoundTask) -> Option<String> {
    let title = clean_title(&found.title)?;
    let source = clean_source(found.source.as_deref());
    Some(task_key(&source, &title, clean_url(found.url.as_deref())))
}

/// Same item from the same place: by link when there is one, else by title.
fn task_key(source: &str, title: &str, url: Option<&str>) -> String {
    match url {
        Some(url) => format!("url:{url}"),
        None => format!("{}:{}", source.to_lowercase(), title.to_lowercase()),
    }
}

/// A link without its query, fragment or trailing slash: the same message,
/// issue or pull request however it was linked.
fn link_key(url: &str) -> String {
    let base = url.split(['?', '#']).next().unwrap_or(url);
    base.trim_end_matches('/').to_lowercase()
}

fn clean_title(title: &str) -> Option<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return None;
    }
    Some(title.chars().take(MAX_TITLE_CHARS).collect())
}

fn clean_source(source: Option<&str>) -> String {
    source
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Claude")
        .to_string()
}

fn clean_url(url: Option<&str>) -> Option<&str> {
    url.map(str::trim).filter(|u| !u.is_empty())
}

/// Splits Claude's answer into the text to show and the body of its fenced
/// block with info string `fence`, if there is a complete one.
pub fn take_fence(answer: &str, fence: &str) -> (String, Option<String>) {
    let opening = format!("```{fence}");
    let Some(start) = answer.find(&opening) else {
        return (answer.to_string(), None);
    };
    let body_start = start + opening.len();
    let Some(body_len) = answer[body_start..].find("```") else {
        return (answer.to_string(), None);
    };
    let body = &answer[body_start..body_start + body_len];
    let block_end = body_start + body_len + 3;
    let shown = format!("{}{}", &answer[..start], &answer[block_end..]);
    (shown.trim().to_string(), Some(body.trim().to_string()))
}

/// Splits Claude's answer into the text to show and the tasks it reported.
pub fn extract(answer: &str) -> (String, Vec<FoundTask>) {
    let (shown, body) = take_fence(answer, TASKS_FENCE);
    let found = body
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default();
    (shown, found)
}

/// The first message of a task's conversation: the task, then what the user asked.
pub fn chat_prompt(task: &Task, message: &str) -> String {
    let link = task.url.as_deref().unwrap_or("none");
    format!(
        "Let's talk about this task from my task list.\nTask: {}\nSource: {}\nLink: {link}\n\n\
         You can look it up through my connectors if you need details.\n\n{message}",
        task.title, task.source
    )
}

/// What the user says when they tick a task as done and want the person who
/// asked to know: Claude drafts the reply where the request was made, never sends it.
pub const DONE_REPLY_REQUEST: &str = "Done. Draft my reply to say so.";

/// The instruction behind [`DONE_REPLY_REQUEST`].
pub fn done_reply_prompt() -> String {
    format!(
        "{DONE_REPLY_REQUEST}\n\nI have done what this task asks. Find the original request \
         (the Slack message or thread, the email, the issue) and who made it, then prepare a \
         short reply in my name saying it is done, with the useful detail (new time, link, \
         result), in the language of the conversation. Leave it as a draft where the request \
         was made: a Slack draft in the same DM or thread (`slack_send_message_draft`, with \
         `thread_ts` for a thread), or a Gmail draft in the same thread. Never send or post \
         anything. If you cannot tell who asked or where, ask me instead of guessing. Then tell \
         me in one line where the draft is."
    )
}

/// Tells a quick action which tasks I have dealt with, so it leaves them out;
/// empty when there are none.
pub fn handled_instruction(handled: &[&Task]) -> String {
    if handled.is_empty() {
        return String::new();
    }
    let handled: Vec<_> = handled
        .iter()
        .map(|t| serde_json::json!({"title": t.title, "source": t.source, "url": t.url}))
        .collect();
    format!(
        "I have already dealt with these, ticked off in my task list: leave them out of your \
         answer, including the same message, issue or pull request under another title or \
         link, unless something new happened since (JSON):\n{}",
        serde_json::Value::Array(handled)
    )
}

/// Asks Claude to report the actionable items of its answer as tasks.
pub fn report_instruction() -> String {
    format!(
        "At the very end of your answer, add a fenced code block with the info string \
         `{TASKS_FENCE}` holding a JSON array of the things I need to act on (not meetings or \
         FYIs), one object each: {{\"title\": short imperative title, \"source\": where it comes \
         from (Linear, GitHub, Slack, Gmail…), \"url\": its link or null}}. {SLACK_NO_REPLY_RULE}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(title: &str, source: &str, url: Option<&str>) -> FoundTask {
        FoundTask {
            title: title.into(),
            source: Some(source.into()),
            url: url.map(Into::into),
        }
    }

    fn titles(list: &TaskList) -> Vec<&str> {
        list.display().iter().map(|t| t.title.as_str()).collect()
    }

    #[test]
    fn report_leaves_out_slack_messages_that_need_no_answer() {
        assert!(report_instruction().contains(SLACK_NO_REPLY_RULE));
        assert!(SLACK_NO_REPLY_RULE.contains("super merci"));
    }

    #[test]
    fn extract_strips_the_block_and_parses_tasks() {
        let answer = "## Pending\n- ACC-52\n\n```meow-tasks\n[{\"title\":\"Fix ACC-52\",\"source\":\"Linear\",\"url\":\"https://l/52\"}]\n```\n";
        let (shown, tasks) = extract(answer);
        assert_eq!(shown, "## Pending\n- ACC-52");
        assert_eq!(
            tasks,
            vec![found("Fix ACC-52", "Linear", Some("https://l/52"))]
        );
    }

    #[test]
    fn extract_leaves_answers_without_block_alone() {
        assert_eq!(extract("hello"), ("hello".into(), vec![]));
        assert_eq!(extract("```meow-tasks\n[] no end").1, vec![]);
    }

    #[test]
    fn extract_tolerates_invalid_json() {
        let (shown, tasks) = extract("text\n```meow-tasks\nnot json\n```");
        assert_eq!(shown, "text");
        assert!(tasks.is_empty());
    }

    #[test]
    fn merge_adds_new_tasks_and_keeps_done_state() {
        let mut list = TaskList::default();
        assert_eq!(
            list.merge(&[found("Review PR", "GitHub", Some("https://g/1"))], 10),
            1
        );
        let id = list.display()[0].id.clone();
        list.set_done(&id, true, 20);

        let again = [
            found("Review PR #1", "GitHub", Some("https://g/1")),
            found("Reply to Maher", "Slack", None),
        ];
        assert_eq!(list.merge(&again, 30), 1);
        let task = list.get(&id).unwrap();
        assert!(task.is_done(), "done state survives a refresh");
        assert_eq!(task.title, "Review PR #1");
        assert_eq!(titles(&list), ["Reply to Maher", "Review PR #1"]);
    }

    #[test]
    fn id_for_matches_the_id_given_on_merge() {
        let mut list = TaskList::default();
        let items = [
            found(" Review  PR ", "GitHub", Some(" https://g/1 ")),
            found("Reply to Bo", "Slack", None),
        ];
        list.merge(&items, 1);
        for item in &items {
            let id = id_for(item).unwrap();
            assert!(list.get(&id).is_some(), "{id}");
        }
        assert_eq!(id_for(&found(" ", "Slack", None)), None);
    }

    #[test]
    fn tasks_without_link_match_by_source_and_title() {
        let mut list = TaskList::default();
        list.merge(&[found("Reply to Maher", "Slack", None)], 1);
        assert_eq!(list.merge(&[found("reply to  maher", "slack", None)], 2), 0);
        assert_eq!(list.merge(&[found("Reply to Maher", "Gmail", None)], 2), 1);
    }

    #[test]
    fn merge_skips_empty_titles_and_defaults_the_source() {
        let mut list = TaskList::default();
        let items = [
            FoundTask {
                title: "  ".into(),
                source: None,
                url: None,
            },
            FoundTask {
                title: "Do it".into(),
                source: None,
                url: Some(" ".into()),
            },
        ];
        assert_eq!(list.merge(&items, 1), 1);
        let task = list.display()[0];
        assert_eq!(
            (task.source.as_str(), task.url.as_deref()),
            ("Claude", None)
        );
    }

    fn group_summary(list: &TaskList) -> Vec<(String, Vec<String>, usize)> {
        list.groups()
            .iter()
            .map(|g| {
                let titles = g.tasks.iter().map(|t| t.title.clone()).collect();
                (g.source.to_string(), titles, g.open_count())
            })
            .collect()
    }

    #[test]
    fn tasks_are_grouped_by_source_in_order_of_first_open_task() {
        let mut list = TaskList::default();
        list.merge(
            &[
                found("PR 1", "GitHub", Some("g1")),
                found("Maher", "Slack", None),
                found("PR 2", "GitHub", Some("g2")),
            ],
            1,
        );
        list.add_manual("Milk", 1);
        let pr1 = list.groups()[0].tasks[0].id.clone();
        list.set_done(&pr1, true, 2);
        assert_eq!(
            group_summary(&list),
            [
                ("GitHub".into(), vec!["PR 2".into()], 1),
                ("Slack".into(), vec!["Maher".into()], 1),
                ("Me".into(), vec!["Milk".into()], 1),
            ]
        );
    }

    #[test]
    fn done_tasks_leave_their_group_for_the_completed_list() {
        let mut list = TaskList::default();
        list.merge(
            &[
                found("PR", "GitHub", Some("g")),
                found("Maher", "Slack", None),
            ],
            1,
        );
        let pr = list.groups()[0].tasks[0].id.clone();
        list.set_done(&pr, true, 2);
        let sources: Vec<_> = list.groups().iter().map(|g| g.source.to_string()).collect();
        assert_eq!(sources, ["Slack"]);
        let maher = list.groups()[0].tasks[0].id.clone();
        list.set_done(&maher, true, 3);
        assert!(list.groups().is_empty());
        let done: Vec<_> = list.completed().iter().map(|t| t.title.clone()).collect();
        assert_eq!(done, ["Maher", "PR"], "most recently done first");
        assert_eq!(list.completed()[1].done_at, Some(2));
    }

    #[test]
    fn empty_list_has_no_groups() {
        assert!(TaskList::default().groups().is_empty());
    }

    #[test]
    fn manual_tasks_are_added_and_trimmed() {
        let mut list = TaskList::default();
        assert!(list.add_manual("  Buy   milk ", 5));
        assert!(!list.add_manual("   ", 5));
        assert_eq!(titles(&list), ["Buy milk"]);
        assert_eq!(list.display()[0].source, MANUAL_SOURCE);
    }

    #[test]
    fn manual_ids_stay_unique_after_pruning() {
        let mut list = TaskList::default();
        list.add_manual("a", 0);
        list.add_manual("b", 0);
        let a = list.display()[0].id.clone();
        list.set_done(&a, true, 0);
        list.prune(DONE_TTL_SECS);
        list.add_manual("c", 0);
        let ids: std::collections::HashSet<_> = list.display().iter().map(|t| &t.id).collect();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn chat_prompt_gives_the_task_then_the_message() {
        let mut list = TaskList::default();
        list.merge(&[found("Fix ACC-52", "Linear", Some("https://l/52"))], 1);
        let prompt = chat_prompt(list.display()[0], "What's blocking it?");
        assert!(prompt.contains("Task: Fix ACC-52"));
        assert!(prompt.contains("Source: Linear"));
        assert!(prompt.contains("Link: https://l/52"));
        assert!(prompt.ends_with("What's blocking it?"));
    }

    #[test]
    fn task_session_is_saved_with_the_task() {
        let dir = std::env::temp_dir().join(format!("meow-session-{}", std::process::id()));
        let path = dir.join("tasks.json");
        let mut list = TaskList::default();
        list.add_manual("Write report", 1);
        let id = list.display()[0].id.clone();
        list.set_session(&id, Some("s1"));
        list.save(&path).unwrap();
        let loaded = TaskList::load(&path).unwrap();
        assert_eq!(loaded.get(&id).unwrap().session.as_deref(), Some("s1"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn background_check_ticks_off_only_open_known_tasks() {
        let mut list = TaskList::default();
        list.merge(
            &[
                found("PR", "GitHub", Some("g1")),
                found("Bo", "Slack", None),
            ],
            1,
        );
        list.add_manual("Milk", 1);
        let milk = list.display()[2].id.clone();
        list.set_done(&milk, true, 2);
        let ids = vec!["url:g1".to_string(), milk.clone(), "unknown".to_string()];
        assert_eq!(list.close_resolved(&ids, 3), 1);
        assert!(
            titles(&list).iter().all(|t| *t != "PR"),
            "resolved task is removed"
        );
        let pr = list.get("url:g1").unwrap();
        assert!(pr.is_done() && pr.auto_done);
        assert!(
            !list.get(&milk).unwrap().auto_done,
            "ticked by hand stays so"
        );
        assert_eq!(list.open().len(), 1);
    }

    #[test]
    fn a_removed_task_is_not_listed_again_and_not_reported_again() {
        let mut list = TaskList::default();
        let pr = found("PR", "GitHub", Some("g1"));
        list.merge(std::slice::from_ref(&pr), 1);
        list.close_resolved(&["url:g1".to_string()], 2);
        list.set_done("url:g1", false, 3);
        assert!(list.display().is_empty());
        assert_eq!(list.merge(std::slice::from_ref(&pr), 4), 0);
        assert!(list.find(&pr).is_some());
        assert!(list.recently_done().iter().any(|t| t.id == "url:g1"));
    }

    #[test]
    fn removed_tasks_are_forgotten_after_the_ttl() {
        let mut list = TaskList::default();
        list.merge(&[found("PR", "GitHub", Some("g1"))], 1);
        list.close_resolved(&["url:g1".to_string()], 2);
        list.prune(2 + DONE_TTL_SECS);
        assert!(list.get("url:g1").is_none());
    }

    #[test]
    fn a_done_task_found_again_under_another_link_stays_done() {
        let mut list = TaskList::default();
        let message = "https://x.slack.com/archives/C1/p1700000000000100";
        list.merge(&[found("Reply to Manon", "Slack", Some(message))], 1);
        let id = list.display()[0].id.clone();
        list.set_done(&id, true, 2);
        let thread = format!("{message}?thread_ts=1700000000.000100&cid=C1");
        let again = found("Reply to Manon's answer", "Slack", Some(&thread));
        assert_eq!(list.merge(std::slice::from_ref(&again), 3), 0);
        assert!(list.find(&again).unwrap().is_done());
        assert_eq!(list.open_count(), 0);
    }

    #[test]
    fn tasks_without_links_match_by_source_and_title() {
        let mut list = TaskList::default();
        list.merge(&[found("Reply to Bo", "Slack", None)], 1);
        let same = found("reply to bo", "slack", None);
        assert_eq!(list.merge(std::slice::from_ref(&same), 2), 0);
        assert!(list.find(&same).is_some());
        assert!(list.find(&found("Reply to Bo", "Gmail", None)).is_none());
    }

    #[test]
    fn done_tasks_go_last_and_can_be_reopened() {
        let mut list = TaskList::default();
        list.add_manual("a", 1);
        list.add_manual("b", 1);
        let a = list.display()[0].id.clone();
        list.set_done(&a, true, 2);
        assert_eq!(titles(&list), ["b", "a"]);
        assert_eq!(list.open_count(), 1);
        list.set_done(&a, false, 3);
        assert_eq!(titles(&list), ["a", "b"]);
    }

    #[test]
    fn prune_forgets_tasks_done_long_ago() {
        let mut list = TaskList::default();
        list.add_manual("old", 0);
        list.add_manual("recent", 0);
        list.add_manual("open", 0);
        let ids: Vec<String> = list.display().iter().map(|t| t.id.clone()).collect();
        list.set_done(&ids[0], true, 100);
        list.set_done(&ids[1], true, DONE_TTL_SECS + 50);
        list.prune(DONE_TTL_SECS + 200);
        assert_eq!(titles(&list), ["open", "recent"]);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("meow-tasks-{}", std::process::id()));
        let path = dir.join("tasks.json");
        let mut list = TaskList::default();
        list.merge(&[found("Fix", "Linear", Some("https://l/1"))], 7);
        list.save(&path).unwrap();
        assert_eq!(TaskList::load(&path).unwrap(), list);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_an_empty_list_and_corrupt_file_an_error() {
        let dir = std::env::temp_dir().join(format!("meow-tasks-bad-{}", std::process::id()));
        assert_eq!(
            TaskList::load(&dir.join("none.json")).unwrap(),
            TaskList::default()
        );
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.json");
        std::fs::write(&path, "{oops").unwrap();
        assert!(TaskList::load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
