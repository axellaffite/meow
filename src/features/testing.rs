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
    pub(crate) menu_changes: usize,
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

    fn menu_changed(&mut self) {
        self.menu_changes += 1;
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

/// The entries as lines: `label: …`, `action: …` (`[x]` when checked), `---`,
/// and a submenu's title then its entries, indented.
pub(crate) fn describe(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| match entry {
            MenuEntry::Label(text) => vec![format!("label: {text}")],
            MenuEntry::Separator => vec!["---".to_string()],
            MenuEntry::Action { title, checked, .. } => {
                let mark = if *checked { "[x] " } else { "" };
                vec![format!("{mark}action: {title}")]
            }
            MenuEntry::Submenu { title, entries } => std::iter::once(format!("{title}:"))
                .chain(
                    describe(entries)
                        .into_iter()
                        .map(|line| format!("  {line}")),
                )
                .collect(),
        })
        .collect()
}

/// Chooses the item at `path`: submenu titles, then the item's title.
pub(crate) fn click(entries: &[MenuEntry], path: &[&str]) {
    let Some((first, rest)) = path.split_first() else {
        panic!("empty menu path");
    };
    for entry in entries {
        match entry {
            MenuEntry::Action { title, run, .. } if title == first && rest.is_empty() => {
                return run();
            }
            MenuEntry::Submenu { title, entries } if title == first => {
                return click(entries, rest);
            }
            _ => {}
        }
    }
    panic!("no menu item {path:?} in {:#?}", describe(entries));
}
