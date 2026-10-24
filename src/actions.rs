//! Quick actions: canned requests offered on the cat's menu and in the chat.
//!
//! Defaults can be replaced by a JSON file: `[{"title": "...", "prompt": "..."}]`,
//! with optional `"daily_at": "07:00"` and `"layout": "plan"`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::schedule::DailyTime;
use crate::tasks::{SLACK_NO_REPLY_RULE, Task};

const MAX_ACTIONS: usize = 12;

const READ_ONLY: &str = "Only read: do not send, post, reply, create or modify anything.";

/// How an action's result is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// The answer as text, in the chat.
    #[default]
    Text,
    /// A day plan page: agenda, focus, replies, with checkable items.
    Plan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickAction {
    pub title: String,
    pub prompt: String,
    pub layout: Layout,
    /// Runs on its own once a day at this time (or as soon as possible after).
    pub daily_at: Option<DailyTime>,
}

impl QuickAction {
    /// The prompt sent to Claude: the action's own text, the tasks I have
    /// already dealt with, and the request to report actionable items, as
    /// tasks or as a day plan whose items become tasks.
    pub fn full_prompt(&self, now: u64, handled: &[&Task]) -> String {
        let report = match self.layout {
            Layout::Text => crate::tasks::report_instruction(),
            Layout::Plan => crate::plan::instruction(now),
        };
        let handled = crate::tasks::handled_instruction(handled);
        if handled.is_empty() {
            format!("{}\n\n{report}", self.prompt)
        } else {
            format!("{}\n\n{handled}\n\n{report}", self.prompt)
        }
    }

    fn new(title: &str, prompt: &str) -> Self {
        Self {
            title: title.to_string(),
            prompt: prompt.to_string(),
            layout: Layout::Text,
            daily_at: None,
        }
    }
}

/// A prompt that has Claude check each source in its own parallel sub-agent,
/// then combine their findings.
fn fan_out(goal: &str, sources: &[(&str, &str)], synthesis: &str) -> String {
    let list: String = sources
        .iter()
        .map(|(name, task)| format!("\n- {name}: {task}"))
        .collect();
    format!(
        "{goal} Launch one sub-agent per source below, all in parallel with the Agent tool \
         (one message, several Agent calls). Tell each one to only read, and to return a short \
         bullet summary with links:{list}\n\nWhen they are all done, {synthesis} If a source \
         fails or is unavailable, say so in one line and carry on. {READ_ONLY}"
    )
}

const LINEAR: (&str, &str) = (
    "Linear",
    "my issues that are not done, overdue or in progress first, with status and due date.",
);
const GITHUB: (&str, &str) = (
    "GitHub",
    "pull requests waiting for my review, and my open pull requests with failing checks, \
     requested changes or unresolved comments.",
);
/// The Slack source's task, completed with [`SLACK_NO_REPLY_RULE`].
const SLACK_OWED: &str = "DMs and threads or mentions since yesterday where I owe a reply. A \
     later message of mine in the same DM or channel, in the thread or not, or a reaction, counts \
     as a reply.";
const GMAIL: (&str, &str) = (
    "Gmail",
    "unread or important emails from the last day, and emails waiting for my answer.",
);

/// Today's planning runs by itself at this time.
const MORNING_AT: DailyTime = DailyTime::new(7, 0);

pub const TODAYS_PLANNING: &str = "Today's planning";
/// The former title of Today's planning, whose saved result is carried over.
pub const LEGACY_PLANNING: &str = "Morning planning";

pub fn defaults() -> Vec<QuickAction> {
    let slack_task = format!("{SLACK_OWED} {SLACK_NO_REPLY_RULE}");
    let slack = ("Slack", slack_task.as_str());
    vec![
        QuickAction {
            layout: Layout::Plan,
            daily_at: Some(MORNING_AT),
            ..QuickAction::new(
                TODAYS_PLANNING,
                &fan_out(
                    "Plan my day.",
                    &[
                        (
                            "Google Calendar",
                            "today's events on my primary calendar only, with times and \
                             attendees. Skip the other calendars I subscribe to, such as \
                             meeting rooms: their events are not mine.",
                        ),
                        LINEAR,
                        GITHUB,
                        slack,
                        GMAIL,
                    ],
                    "combine their findings into my plan for the day: the agenda, the 3 \
                 priorities to tackle first and why, what needs a reply, and the rest to act on.",
                ),
            )
        },
        QuickAction::new(
            "Pending tasks",
            &fan_out(
                "List everything waiting on me.",
                &[LINEAR, GITHUB, slack, GMAIL],
                "group the items by source, most urgent first, one line per item with a link \
                 when there is one.",
            ),
        ),
    ]
}

/// `~/.config/meow/actions.json`.
pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/meow/actions.json"))
}

/// Actions from `path`, or the defaults when the file does not exist.
pub fn load(path: &Path) -> Result<Vec<QuickAction>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(defaults()),
        Err(e) => Err(format!("Could not read {}: {e}", path.display())),
    }
}

fn parse(text: &str) -> Result<Vec<QuickAction>, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("invalid JSON ({e})"))?;
    let entries = value
        .as_array()
        .ok_or("expected a list of {\"title\", \"prompt\"} objects")?;
    if entries.len() > MAX_ACTIONS {
        return Err(format!("at most {MAX_ACTIONS} actions are supported"));
    }
    entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let field = |key: &str| {
                entry
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("action {} needs a non-empty \"{key}\"", i + 1))
            };
            let action = QuickAction::new(field("title")?, field("prompt")?);
            Ok(QuickAction {
                layout: parse_layout(entry, i)?,
                daily_at: parse_daily_at(entry, i)?,
                ..action
            })
        })
        .collect()
}

fn parse_layout(entry: &Value, i: usize) -> Result<Layout, String> {
    match entry.get("layout").map(|v| v.as_str()) {
        None | Some(Some("text")) => Ok(Layout::Text),
        Some(Some("plan")) => Ok(Layout::Plan),
        Some(_) => Err(format!(
            "action {}: \"layout\" is \"text\" or \"plan\"",
            i + 1
        )),
    }
}

fn parse_daily_at(entry: &Value, i: usize) -> Result<Option<DailyTime>, String> {
    match entry.get("daily_at") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .and_then(DailyTime::parse)
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "action {}: \"daily_at\" must be a time like \"07:00\"",
                    i + 1
                )
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_offer_todays_planning_and_pending_tasks() {
        let titles: Vec<_> = defaults().into_iter().map(|a| a.title).collect();
        assert_eq!(titles, ["Today's planning", "Pending tasks"]);
    }

    #[test]
    fn default_prompts_are_read_only() {
        assert!(defaults().iter().all(|a| a.prompt.contains(READ_ONLY)));
    }

    #[test]
    fn default_prompts_fan_out_one_agent_per_source() {
        for action in defaults() {
            assert!(action.prompt.contains("Agent tool"), "{}", action.title);
            for source in ["Linear", "GitHub", "Slack", "Gmail"] {
                assert!(action.prompt.contains(&format!("- {source}:")), "{source}");
            }
        }
        assert!(defaults()[0].prompt.contains("- Google Calendar:"));
    }

    #[test]
    fn slack_source_skips_messages_that_need_no_answer() {
        for action in defaults() {
            assert!(
                action.prompt.contains(SLACK_NO_REPLY_RULE),
                "{}",
                action.title
            );
        }
    }

    #[test]
    fn every_action_asks_claude_to_report_tasks() {
        let custom = QuickAction::new("Inbox", "Summarize my inbox");
        assert!(custom.full_prompt(0, &[]).starts_with("Summarize my inbox"));
        assert!(
            custom
                .full_prompt(0, &[])
                .contains(crate::tasks::TASKS_FENCE)
        );
    }

    #[test]
    fn actions_leave_out_tasks_already_dealt_with() {
        let mut list = crate::tasks::TaskList::default();
        list.add_manual("Reply to Marine", 0);
        let id = list.display()[0].id.clone();
        list.set_done(&id, true, 1);
        let action = QuickAction::new("Inbox", "Summarize my inbox");
        let prompt = action.full_prompt(0, &list.recently_done());
        assert!(prompt.contains("already dealt with"));
        assert!(prompt.contains("Reply to Marine"));
        assert!(!action.full_prompt(0, &[]).contains("already dealt with"));
    }

    #[test]
    fn todays_planning_is_a_daily_plan_at_seven() {
        let morning = &defaults()[0];
        assert_eq!(morning.layout, Layout::Plan);
        assert_eq!(morning.daily_at, Some(DailyTime::new(7, 0)));
        let prompt = morning.full_prompt(0, &[]);
        assert!(prompt.contains(crate::plan::PLAN_FENCE));
        assert!(!prompt.contains(crate::tasks::TASKS_FENCE));
        assert_eq!(defaults()[1].daily_at, None);
    }

    #[test]
    fn parses_schedule_and_layout() {
        let text = r#"[{"title": "Plan", "prompt": "p", "daily_at": "8:30", "layout": "plan"}]"#;
        let action = &parse(text).unwrap()[0];
        assert_eq!(action.daily_at, Some(DailyTime::new(8, 30)));
        assert_eq!(action.layout, Layout::Plan);
        let bad_time = r#"[{"title": "A", "prompt": "p", "daily_at": "25:00"}]"#;
        assert!(parse(bad_time).unwrap_err().contains("daily_at"));
        let bad_layout = r#"[{"title": "A", "prompt": "p", "layout": "grid"}]"#;
        assert!(parse(bad_layout).unwrap_err().contains("layout"));
    }

    #[test]
    fn parses_custom_actions() {
        let text = r#"[{"title": " Inbox ", "prompt": "Summarize my inbox"}]"#;
        assert_eq!(
            parse(text).unwrap(),
            vec![QuickAction::new("Inbox", "Summarize my inbox")]
        );
    }

    #[test]
    fn rejects_missing_or_empty_fields() {
        let error = parse(r#"[{"title": "A", "prompt": "p"}, {"title": ""}]"#).unwrap_err();
        assert!(error.contains("action 2"), "{error}");
    }

    #[test]
    fn rejects_non_list_and_too_many_actions() {
        assert!(parse(r#"{"title": "A"}"#).is_err());
        let many = format!("[{}]", vec![r#"{"title":"a","prompt":"b"}"#; 13].join(","));
        assert!(parse(&many).is_err());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let path = std::env::temp_dir().join("meow-no-such-actions.json");
        assert_eq!(load(&path).unwrap(), defaults());
    }

    #[test]
    fn invalid_file_reports_its_path() {
        let path = std::env::temp_dir().join(format!("meow-actions-{}.json", std::process::id()));
        std::fs::write(&path, "not json").unwrap();
        let error = load(&path).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(
            error.contains("meow-actions-") && error.contains("invalid JSON"),
            "{error}"
        );
    }
}
