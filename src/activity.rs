//! The activity log: what happened in the background (Slack messages for me,
//! calendar changes, tasks found or resolved), newest first, for the Activity
//! page. Items are kept a week, or until dismissed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::source::Source;

/// Items are forgotten this long after they happened.
pub const KEEP_SECS: u64 = 7 * 24 * 60 * 60;
const MAX_ITEMS: usize = 200;
const MAX_TEXT_CHARS: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Slack,
    Calendar,
    Tasks,
    /// A scheduled task ran, or a watch fired.
    Scheduled,
    /// Autopilot work finished, or waits for approval.
    Autopilot,
}

impl Kind {
    /// "SLACK", for the item's tag.
    pub fn label(self) -> &'static str {
        match self {
            Self::Slack => "SLACK",
            Self::Calendar => "CALENDAR",
            Self::Tasks => "TASKS",
            Self::Scheduled => "SCHEDULED",
            Self::Autopilot => "AUTOPILOT",
        }
    }

    /// Where an item of this kind comes from when it does not say.
    fn default_sources(self) -> Vec<Source> {
        match self {
            Self::Slack => vec![Source::Slack],
            Self::Calendar => vec![Source::Calendar],
            Self::Tasks => vec![Source::Other],
            Self::Scheduled => vec![Source::Scheduled],
            Self::Autopilot => vec![Source::Slack],
        }
    }
}

/// Where clicking an item goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", rename_all = "lowercase")]
pub enum Link {
    /// A Slack conversation, opened on the Slack page at one message.
    Slack {
        channel: String,
        /// The message to show; `None` opens on the latest.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ts: Option<String>,
        /// Its thread, shown instead when the message itself is not loaded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread_ts: Option<String>,
    },
    /// The plan of this quick action.
    Plan {
        action: String,
    },
    Tasks,
    Autopilot,
    Url {
        url: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub id: u64,
    pub kind: Kind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub at: u64,
    #[serde(default)]
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Link>,
    /// Where the news comes from; empty means its kind's usual source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    /// A new item with the same key replaces this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl Activity {
    /// Where the news comes from, for its icons.
    pub fn sources(&self) -> Vec<Source> {
        if self.sources.is_empty() {
            self.kind.default_sources()
        } else {
            self.sources.clone()
        }
    }
}

/// What a new item says, before it joins the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub kind: Kind,
    pub title: String,
    pub detail: Option<String>,
    pub link: Option<Link>,
    /// Empty for the kind's usual source.
    pub sources: Vec<Source>,
    /// Replaces the item already logged under this key, e.g. one per Slack
    /// sender and conversation so only their last message shows.
    pub key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityLog {
    /// Newest first.
    items: Vec<Activity>,
    #[serde(default)]
    next_id: u64,
}

impl ActivityLog {
    pub fn items(&self) -> &[Activity] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&Activity> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Adds `note` on top, unread, in place of any item with its key;
    /// `None` when it has no title.
    pub fn push(&mut self, note: Note, now: u64) -> Option<&Activity> {
        let title = clean(&note.title)?;
        if let Some(key) = &note.key {
            self.items.retain(|item| item.key.as_ref() != Some(key));
        }
        let item = Activity {
            id: self.next_id,
            kind: note.kind,
            title,
            detail: note.detail.as_deref().and_then(clean),
            at: now,
            read: false,
            link: note.link,
            sources: note.sources,
            key: note.key,
        };
        self.next_id += 1;
        self.items.insert(0, item);
        self.items.truncate(MAX_ITEMS);
        self.items.first()
    }

    pub fn unread_count(&self) -> usize {
        self.items.iter().filter(|item| !item.read).count()
    }

    /// Marks everything read; `true` if anything changed.
    pub fn mark_all_read(&mut self) -> bool {
        let changed = self.unread_count() > 0;
        for item in &mut self.items {
            item.read = true;
        }
        changed
    }

    /// Marks the items `about` picks read; `true` if any was unread.
    pub fn mark_read_where(&mut self, about: impl Fn(&Activity) -> bool) -> bool {
        let mut changed = false;
        for item in self
            .items
            .iter_mut()
            .filter(|item| !item.read && about(item))
        {
            item.read = true;
            changed = true;
        }
        changed
    }

    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() != before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Forgets items older than [`KEEP_SECS`]; `true` if any went.
    pub fn prune(&mut self, now: u64) -> bool {
        let before = self.items.len();
        self.items
            .retain(|item| now.saturating_sub(item.at) < KEEP_SECS);
        self.items.len() != before
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("Could not read activity from {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Could not read {}: {e}", path.display())),
        }
    }

    /// Writes atomically, so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let fail = |e: &dyn std::fmt::Display| {
            format!("Could not save activity to {}: {e}", path.display())
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| fail(&e))?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, json).map_err(|e| fail(&e))?;
        std::fs::rename(&temp, path).map_err(|e| fail(&e))
    }
}

/// `~/Library/Application Support/Meow/activity.json`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/Meow/activity.json"))
}

/// "A, B and 2 more", for items that group several things.
pub fn list_titles(titles: &[&str], shown: usize) -> String {
    match titles.len() {
        0 => String::new(),
        n if n <= shown => titles.join(", "),
        n => format!("{} and {} more", titles[..shown].join(", "), n - shown),
    }
}

fn clean(text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| text.chars().take(MAX_TEXT_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str) -> Note {
        Note {
            kind: Kind::Tasks,
            title: title.into(),
            detail: Some("  ".into()),
            link: Some(Link::Tasks),
            sources: Vec::new(),
            key: None,
        }
    }

    #[test]
    fn new_items_go_on_top_unread_and_trimmed() {
        let mut log = ActivityLog::default();
        log.push(note("first"), 1);
        let second = log.push(note("  second   one "), 2).unwrap();
        assert_eq!(
            (second.title.as_str(), second.detail.as_ref()),
            ("second one", None)
        );
        let titles: Vec<_> = log.items().iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["second one", "first"]);
        assert_eq!(log.unread_count(), 2);
        assert!(log.push(note(" "), 3).is_none());
    }

    #[test]
    fn reading_dismissing_and_clearing() {
        let mut log = ActivityLog::default();
        let a = log.push(note("a"), 1).unwrap().id;
        log.push(note("b"), 2);
        assert!(log.mark_all_read());
        assert!(!log.mark_all_read(), "nothing left to mark");
        assert!(log.dismiss(a) && !log.dismiss(a));
        assert_eq!(log.items().len(), 1);
        log.clear();
        assert!(log.items().is_empty());
    }

    #[test]
    fn some_items_can_be_marked_read() {
        let mut log = ActivityLog::default();
        log.push(note("a"), 1);
        log.push(note("b"), 2);
        assert!(log.mark_read_where(|item| item.title == "a"));
        assert!(
            !log.mark_read_where(|item| item.title == "a"),
            "already read"
        );
        assert_eq!(log.unread_count(), 1);
    }

    #[test]
    fn ids_stay_unique_after_dismissing() {
        let mut log = ActivityLog::default();
        let a = log.push(note("a"), 1).unwrap().id;
        log.dismiss(a);
        assert_ne!(log.push(note("b"), 2).unwrap().id, a);
    }

    #[test]
    fn old_items_are_pruned_and_the_log_is_bounded() {
        let mut log = ActivityLog::default();
        log.push(note("old"), 0);
        log.push(note("recent"), KEEP_SECS);
        assert!(log.prune(KEEP_SECS + 10));
        assert_eq!(log.items().len(), 1);
        for n in 0..MAX_ITEMS + 5 {
            log.push(note(&n.to_string()), KEEP_SECS);
        }
        assert_eq!(log.items().len(), MAX_ITEMS);
    }

    #[test]
    fn a_keyed_item_replaces_the_older_one_with_its_key() {
        let mut log = ActivityLog::default();
        let keyed = |title: &str, key: &str| Note {
            key: Some(key.into()),
            ..note(title)
        };
        log.push(keyed("Bo: hi", "slack:D1:bo"), 1);
        log.push(keyed("Al: yo", "slack:D1:al"), 2);
        log.push(note("unkeyed"), 3);
        log.push(keyed("Bo: still there?", "slack:D1:bo"), 4);
        let titles: Vec<_> = log.items().iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Bo: still there?", "unkeyed", "Al: yo"]);
    }

    #[test]
    fn items_without_sources_use_their_kinds() {
        let mut log = ActivityLog::default();
        log.push(note("found"), 1);
        assert_eq!(log.items()[0].sources(), [Source::Other]);
        log.push(
            Note {
                sources: vec![Source::GitHub, Source::Linear],
                ..note("found")
            },
            2,
        );
        assert_eq!(log.items()[0].sources(), [Source::GitHub, Source::Linear]);
        let saved: Activity =
            serde_json::from_str(r#"{"id":1,"kind":"slack","title":"Bo","at":1}"#).unwrap();
        assert_eq!(saved.sources(), [Source::Slack]);
    }

    #[test]
    fn slack_links_saved_without_a_message_still_load() {
        let link: Link = serde_json::from_str(r#"{"to":"slack","channel":"D1"}"#).unwrap();
        assert_eq!(
            link,
            Link::Slack {
                channel: "D1".into(),
                ts: None,
                thread_ts: None
            }
        );
    }

    #[test]
    fn lists_name_a_few_then_count_the_rest() {
        assert_eq!(list_titles(&["a", "b"], 3), "a, b");
        assert_eq!(list_titles(&["a", "b", "c", "d"], 2), "a, b and 2 more");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("meow-activity-{}", std::process::id()));
        let path = dir.join("activity.json");
        let mut log = ActivityLog::default();
        log.push(
            Note {
                kind: Kind::Slack,
                title: "Bo: are you there?".into(),
                detail: None,
                link: Some(Link::Slack {
                    channel: "D1".into(),
                    ts: Some("1729332000.000100".into()),
                    thread_ts: None,
                }),
                sources: vec![Source::Slack],
                key: Some("slack:D1:U1".into()),
            },
            5,
        );
        log.save(&path).unwrap();
        assert_eq!(ActivityLog::load(&path).unwrap(), log);
        assert_eq!(
            ActivityLog::load(&dir.join("missing.json")).unwrap(),
            ActivityLog::default()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
