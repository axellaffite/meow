//! What feature tests share: a `Host` that records what a feature tells the
//! app (no AppKit, no Claude, settings kept in memory), and helpers to read
//! and click a feature's menu entries.

use meow::action_results::ActionResults;
use meow::actions::QuickAction;
use meow::activity::{Activity, Note};
use meow::agent::Proposal;
use meow::claude::ClaudeEnv;
use meow::conversations::ChatId;
use meow::plan::{DayPlan, Event};
use meow::settings::Store;
use meow::slack::calendar::CalendarTools;
use meow::sprites::Coat;
use meow::superset::Project;
use meow::tasks::TaskList;

use crate::app::agents::AgentSession;
use crate::app::feature::Host;
use crate::ui::menus::MenuEntry;
use crate::ui::settings_page::{SettingEntry, SettingsSection};
use crate::ui::sidebar::Page;

#[derive(Default)]
pub(crate) struct FakeHost {
    pub(crate) infos: Vec<(ChatId, String)>,
    pub(crate) errors: Vec<(ChatId, String)>,
    pub(crate) banners: Vec<String>,
    pub(crate) notes: Vec<Note>,
    pub(crate) badges: Vec<(&'static str, usize)>,
    /// `None` (the default): Claude is not ready, and nothing ever runs.
    pub(crate) claude_env: Option<ClaudeEnv>,
    pub(crate) calendar: Option<CalendarTools>,
    pub(crate) actions: Vec<QuickAction>,
    pub(crate) results: ActionResults,
    pub(crate) plans_changes: usize,
    pub(crate) chat_in_use: bool,
    /// Each quick action asked to run, by index.
    pub(crate) ran_actions: Vec<usize>,
    pub(crate) settings: Store,
    pub(crate) tasks: TaskList,
    /// How many times the task list was said to have changed.
    pub(crate) tasks_changes: usize,
    /// Each status shown in the Tasks header.
    pub(crate) tasks_status: Vec<Option<String>>,
    /// Each coat the cat was dressed in.
    pub(crate) worn: Vec<Coat>,
    /// Each size the chat bubble was given.
    pub(crate) sizes: Vec<(f64, f64)>,
    /// Each time the news bubble over the cat was turned on or off.
    pub(crate) news_shown: Vec<bool>,
    pub(crate) settings_changes: usize,
    /// Conversations flagged with `!` over the cat.
    pub(crate) unread: Vec<ChatId>,
    /// Each page asked to open.
    pub(crate) opened: Vec<Page>,
    /// How many times `!` was asked for over the cat.
    pub(crate) alerts: usize,
    pub(crate) idle: bool,
    pub(crate) quit: bool,
}

impl Host for FakeHost {
    fn info_in(&mut self, chat: &ChatId, text: &str) {
        self.infos.push((chat.clone(), text.to_string()));
    }

    fn error_in(&mut self, chat: &ChatId, text: &str) {
        self.errors.push((chat.clone(), text.to_string()));
    }

    fn flag_unread(&mut self, chat: &ChatId) {
        self.unread.push(chat.clone());
    }

    fn alert(&mut self) {
        self.alerts += 1;
    }

    fn read_activity(&mut self, _about: &dyn Fn(&Activity) -> bool) {}

    fn open_page(&mut self, page: Page) {
        self.opened.push(page);
    }

    fn page_visible(&self, _id: &str) -> bool {
        false
    }

    fn superset_ready(&self) -> Result<(), String> {
        Err("no Superset in tests".to_string())
    }

    fn projects(&self) -> &[Project] {
        &[]
    }

    fn discover_superset(&mut self) {}

    fn launch_slack_agent(
        &mut self,
        _channel: &str,
        _request: &str,
        _proposal: &Proposal,
    ) -> Result<(), String> {
        Err("no Superset in tests".to_string())
    }

    fn slack_agent(&self, _channel: &str) -> Option<&AgentSession> {
        None
    }

    fn agents_revision(&self) -> u64 {
        0
    }

    fn notify(&mut self, text: &str) {
        self.banners.push(text.to_string());
    }

    fn record(&mut self, note: Note) {
        self.notes.push(note);
    }

    fn claude(&self) -> Option<&ClaudeEnv> {
        self.claude_env.as_ref()
    }

    fn calendar_tools(&self) -> Option<&CalendarTools> {
        self.calendar.as_ref()
    }

    fn quick_actions(&self) -> &[QuickAction] {
        &self.actions
    }

    fn last_result(&self, title: &str) -> Option<(u64, Option<&DayPlan>)> {
        let result = self.results.get(title)?;
        Some((result.ran_at, result.plan.as_ref()))
    }

    fn set_plan_agenda(&mut self, title: &str, agenda: Vec<Event>) -> bool {
        self.results.set_agenda(title, agenda)
    }

    fn plans_changed(&mut self) {
        self.plans_changes += 1;
    }

    fn chat_in_use(&self) -> bool {
        self.chat_in_use
    }

    fn run_quick_action(&mut self, index: usize) {
        self.ran_actions.push(index);
    }

    fn settings(&mut self) -> &mut Store {
        &mut self.settings
    }

    fn tasks(&mut self) -> &mut TaskList {
        &mut self.tasks
    }

    fn tasks_changed(&mut self) {
        self.tasks_changes += 1;
    }

    fn set_tasks_status(&mut self, status: Option<&str>) {
        self.tasks_status.push(status.map(str::to_string));
    }

    fn wear(&mut self, coat: Coat) {
        self.worn.push(coat);
    }

    fn resize_chat(&mut self, size: (f64, f64)) {
        self.sizes.push(size);
    }

    fn show_news(&mut self, shown: bool) {
        self.news_shown.push(shown);
    }

    fn settings_changed(&mut self) {
        self.settings_changes += 1;
    }

    fn set_badge(&mut self, page: &'static str, count: usize) {
        self.badges.push((page, count));
    }

    fn is_idle(&self) -> bool {
        self.idle
    }

    fn quit(&mut self) {
        self.quit = true;
    }
}

/// The entries as lines: `label: …` and `action: …` (`[x]` when checked).
pub(crate) fn describe(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Label(text) => format!("label: {text}"),
            MenuEntry::Action { title, checked, .. } => {
                let mark = if *checked { "[x] " } else { "" };
                format!("{mark}action: {title}")
            }
        })
        .collect()
}

/// Chooses the item titled `title`.
pub(crate) fn click(entries: &[MenuEntry], title: &str) {
    for entry in entries {
        if let MenuEntry::Action { title: t, run, .. } = entry
            && t == title
        {
            return run();
        }
    }
    panic!("no menu item {title:?} in {:#?}", describe(entries));
}

/// The sections as lines: `# Title`, then each entry: `note: …`,
/// `toggle: …` (`[x]` when on), `choice: Title = A | [B] | C` (the selected
/// one in brackets), `text: Title = value` and `button: …`.
pub(crate) fn describe_settings(sections: &[SettingsSection]) -> Vec<String> {
    sections
        .iter()
        .flat_map(|section| {
            let entries = section.entries.iter().map(|entry| match entry {
                SettingEntry::Note(text) => format!("note: {text}"),
                SettingEntry::Toggle { title, on, .. } => {
                    let mark = if *on { "[x] " } else { "" };
                    format!("{mark}toggle: {title}")
                }
                SettingEntry::Choice {
                    title,
                    options,
                    selected,
                    ..
                } => {
                    let options: Vec<String> = options
                        .iter()
                        .enumerate()
                        .map(|(index, option)| match *selected == Some(index) {
                            true => format!("[{option}]"),
                            false => option.clone(),
                        })
                        .collect();
                    format!("choice: {title} = {}", options.join(" | "))
                }
                SettingEntry::Text { title, value, .. } => format!("text: {title} = {value}"),
                SettingEntry::Button { title, .. } => format!("button: {title}"),
            });
            std::iter::once(format!("# {}", section.title)).chain(entries)
        })
        .collect()
}

/// Uses entry `title` of section `section`: flips a toggle, presses a button,
/// picks option `value` of a choice, or saves `value` as a text's.
pub(crate) fn change_setting(
    sections: &[SettingsSection],
    section: &str,
    title: &str,
    value: &str,
) {
    let entry = sections
        .iter()
        .filter(|s| s.title == section)
        .flat_map(|s| &s.entries)
        .find(|entry| match entry {
            SettingEntry::Note(_) => false,
            SettingEntry::Toggle { title: t, .. }
            | SettingEntry::Choice { title: t, .. }
            | SettingEntry::Text { title: t, .. }
            | SettingEntry::Button { title: t, .. } => t == title,
        });
    match entry {
        Some(SettingEntry::Toggle { run, .. } | SettingEntry::Button { run, .. }) => run(),
        Some(SettingEntry::Choice { options, pick, .. }) => {
            let Some(index) = options.iter().position(|option| option == value) else {
                panic!("no option {value:?} in {title:?}: {options:?}");
            };
            pick(index)
        }
        Some(SettingEntry::Text { save, .. }) => save(value.to_string()),
        Some(SettingEntry::Note(_)) | None => panic!(
            "no setting {section} › {title} in {:#?}",
            describe_settings(sections)
        ),
    }
}
