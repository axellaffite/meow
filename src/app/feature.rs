//! What a feature is to the app. A feature lives in its own folder under
//! `src/features/`, keeps its own state and messages, and reaches the rest of
//! the app only through `Host`; see ARCHITECTURE.md. A new feature adds files
//! and one line in `features::all`, never a field on `App` or a `UiEvent`.

use meow::actions::QuickAction;
use meow::activity::{Activity, Link, Note};
use meow::agent::Proposal;
use meow::claude::ClaudeEnv;
use meow::conversations::ChatId;
use meow::plan::{DayPlan, Event};
use meow::settings::Store;
use meow::slack::calendar::CalendarTools;
use meow::sprites::Coat;
use meow::superset::Project;
use meow::tasks::TaskList;
use objc2::MainThreadMarker;
use objc2::rc::Retained;

use super::App;
use super::agents::{AgentSession, Origin};
use crate::ui::chat_window::{PageKit, PageView};
use crate::ui::controller::Controller;
use crate::ui::menus::{self, MenuEntry, MenuSection};
use crate::ui::notify;
use crate::ui::settings_page::SettingsSection;
use crate::ui::sidebar::{Page, PageSpec};

pub(crate) trait Feature {
    /// Once per tick, after the UI events: handle the feature's own messages
    /// and check on its background work.
    fn poll(&mut self, host: &mut dyn Host);

    /// Why the feature's saved data could not be read, told once at launch.
    fn startup_error(&self) -> Option<String> {
        None
    }

    /// A command typed in conversation `chat`, such as `/update`: whether this
    /// feature takes it. It is acted on at the next poll.
    fn command(&mut self, _text: &str, _chat: &ChatId) -> bool {
        false
    }

    /// Added to the prompt of each message typed in the chat.
    fn chat_instruction(&self, _now: u64) -> Option<String> {
        None
    }

    /// A reply in conversation `chat`, `done` for the final one: the text
    /// without the blocks this feature reads in it. Acted on at the next poll.
    fn read_reply(&mut self, _chat: &ChatId, text: &str, _done: bool) -> String {
        text.to_string()
    }

    /// Items for the cat's menu, in `menu_section`. Asked each time the
    /// menu is filled again.
    fn menu(&self) -> Vec<MenuEntry> {
        Vec::new()
    }

    /// Where `menu`'s items go: above "Quit Meow" unless said otherwise.
    fn menu_section(&self) -> MenuSection {
        MenuSection::App
    }

    /// The feature's sections of the Settings page, in order. Asked again
    /// after the feature calls `Host::settings_changed`.
    fn settings(&self) -> Vec<SettingsSection> {
        Vec::new()
    }

    /// The feature's pages in the sidebar.
    fn pages(&self) -> Vec<PageSpec> {
        Vec::new()
    }

    /// Builds page `id`'s view, in `frame` of the chat bubble. Called once
    /// for each of `pages`, at launch.
    fn build_page(&mut self, _id: &'static str, _kit: &PageKit) -> Option<PageView> {
        None
    }

    /// Page `id` is about to be shown.
    fn page_shown(&mut self, _id: &'static str) {}

    /// An Activity item was clicked: whether this feature shows what `link`
    /// is about.
    fn open_link(&mut self, _link: &Link, _host: &mut dyn Host) -> bool {
        false
    }

    /// Esc was pressed on one of the feature's pages: whether it did
    /// something with it (closing a panel, going back) rather than letting
    /// the bubble close.
    fn escape(&mut self, _host: &mut dyn Host) -> bool {
        false
    }

    /// Meow is quitting: save what must not be lost.
    fn quit(&mut self) {}

    /// The steps the feature adds to snapshot mode, each saved as
    /// `<step>.png`: by default its pages.
    fn snapshot_steps(&self) -> Vec<&'static str> {
        self.pages().iter().map(|page| page.id).collect()
    }

    /// Shows snapshot step `step`, with sample data rather than the user's
    /// when it has any.
    fn show_snapshot(&mut self, step: &'static str, host: &mut dyn Host) {
        host.open_page(Page::Feature(step));
    }

    /// Work under way that a restart would lose.
    fn is_busy(&self) -> bool {
        false
    }

    /// Work the user is waiting on: the cat shows its `…` bubble meanwhile.
    fn thinking(&self) -> bool {
        false
    }
}

/// What the app offers its features.
pub(crate) trait Host {
    /// A note in conversation `chat`.
    fn info_in(&mut self, chat: &ChatId, text: &str);
    fn error_in(&mut self, chat: &ChatId, text: &str);
    /// Shows `!` over the cat until conversation `chat` is seen, unless it is
    /// on screen already.
    fn flag_unread(&mut self, chat: &ChatId);
    /// Shows `!` over the cat until the bubble is opened.
    fn alert(&mut self);
    /// A macOS banner.
    fn notify(&mut self, text: &str);
    /// Something that happened, for the Activity page (with a banner).
    fn record(&mut self, note: Note);
    /// Marks the Activity items `about` matches as read.
    fn read_activity(&mut self, about: &dyn Fn(&Activity) -> bool);
    /// Opens the chat bubble on `page`.
    fn open_page(&mut self, page: Page);
    /// Whether the chat bubble is open on feature page `id`.
    fn page_visible(&self, id: &str) -> bool;
    /// Whether coding agents can be started now, or why not.
    fn superset_ready(&self) -> Result<(), String>;
    /// The Superset projects agents can work in.
    fn projects(&self) -> &[Project];
    /// Looks for Superset and its projects again, in the background.
    fn discover_superset(&mut self);
    /// Starts `proposal`'s coding agent for Slack conversation `channel`.
    fn launch_slack_agent(
        &mut self,
        channel: &str,
        request: &str,
        proposal: &Proposal,
    ) -> Result<(), String>;
    /// The coding agent followed for Slack conversation `channel`.
    fn slack_agent(&self, channel: &str) -> Option<&AgentSession>;
    /// Changes each time a coding agent moves on, to know when to redraw.
    fn agents_revision(&self) -> u64;
    /// The Claude Code setup, once found; `None` before or without one.
    fn claude(&self) -> Option<&ClaudeEnv>;
    /// The Claude Code setup, or why there is none, to tell the user.
    fn claude_or_why(&self) -> Result<&ClaudeEnv, String> {
        self.claude()
            .ok_or_else(|| "Claude is still starting; try again in a moment.".to_string())
    }
    /// The calendar tools of the user's Claude Code, once found.
    fn calendar_tools(&self) -> Option<&CalendarTools>;
    /// The quick actions, as configured.
    fn quick_actions(&self) -> &[QuickAction];
    /// When quick action `title` last ran, and its day plan if it has one.
    fn last_result(&self, title: &str) -> Option<(u64, Option<&DayPlan>)>;
    /// Replaces the agenda of quick action `title`'s day plan; whether that
    /// changed it. After changing plans, call `plans_changed`.
    fn set_plan_agenda(&mut self, title: &str, agenda: Vec<Event>) -> bool;
    /// Saves the quick actions' results and redraws the plan on screen.
    fn plans_changed(&mut self);
    /// A request is running or the chat bubble is open.
    fn chat_in_use(&self) -> bool;
    /// Runs quick action `index` afresh (in the background when the bubble
    /// is closed, with `!` over the cat when done).
    fn run_quick_action(&mut self, index: usize);
    /// The task list. After changing it, call `tasks_changed`.
    fn tasks(&mut self) -> &mut TaskList;
    /// Saves the task list and redraws it.
    fn tasks_changed(&mut self);
    /// The status after "Tasks" in the Tasks page's header, e.g. "checked at 09:40".
    fn set_tasks_status(&mut self, status: Option<&str>);
    /// The settings file; a feature reads and saves only its own fields.
    fn settings(&mut self) -> &mut Store;
    /// Dresses the cat (and the sidebar's little cat) in `coat`.
    fn wear(&mut self, coat: Coat);
    /// The chat bubble's size, in points; it is fitted to the screen.
    fn resize_chat(&mut self, size: (f64, f64));
    /// Whether unread news shows where it comes from in a bubble over the
    /// cat (the default), or only the cat's `!`.
    fn show_news(&mut self, shown: bool);
    /// The feature's settings changed: the Settings page asks for them again.
    fn settings_changed(&mut self);
    /// The count shown at the end of `page`'s row in the sidebar.
    fn set_badge(&mut self, page: &'static str, count: usize);
    /// Nothing the user would lose to a restart: no run, no open chat, no
    /// background work, in the app or in any other feature.
    fn is_idle(&self) -> bool;
    fn quit(&mut self);
}

/// The app, as seen by the feature being polled.
struct AppHost<'a> {
    app: &'a mut App,
    /// Whether another feature has work under way.
    others_busy: bool,
}

impl Host for AppHost<'_> {
    fn info_in(&mut self, chat: &ChatId, text: &str) {
        self.app.info_in(chat, text);
    }

    fn error_in(&mut self, chat: &ChatId, text: &str) {
        self.app.error_in(chat, text);
    }

    fn flag_unread(&mut self, chat: &ChatId) {
        self.app.flag_unread(chat);
    }

    fn alert(&mut self) {
        self.app.unread = true;
    }

    fn notify(&mut self, text: &str) {
        notify::post("Meow", text);
    }

    fn read_activity(&mut self, about: &dyn Fn(&Activity) -> bool) {
        self.app.read_activity(about);
    }

    fn open_page(&mut self, page: Page) {
        self.app.open_page(page);
    }

    fn page_visible(&self, id: &str) -> bool {
        self.app.chat.is_visible()
            && matches!(self.app.chat.page(), Page::Feature(shown) if shown == id)
    }

    fn superset_ready(&self) -> Result<(), String> {
        self.app.superset_ready()
    }

    fn projects(&self) -> &[Project] {
        self.app.projects()
    }

    fn discover_superset(&mut self) {
        self.app.discover_superset();
    }

    fn launch_slack_agent(
        &mut self,
        channel: &str,
        request: &str,
        proposal: &Proposal,
    ) -> Result<(), String> {
        let origin = Origin::Slack {
            channel: channel.to_string(),
        };
        self.app.launch_agent(origin, request, proposal)
    }

    fn slack_agent(&self, channel: &str) -> Option<&AgentSession> {
        self.app.slack_agent(channel)
    }

    fn agents_revision(&self) -> u64 {
        self.app.agents_revision()
    }

    fn settings(&mut self) -> &mut Store {
        &mut self.app.store
    }

    fn calendar_tools(&self) -> Option<&CalendarTools> {
        self.app.calendar.tools()
    }

    fn quick_actions(&self) -> &[QuickAction] {
        &self.app.actions
    }

    fn last_result(&self, title: &str) -> Option<(u64, Option<&DayPlan>)> {
        let result = self.app.action_results.get(title)?;
        Some((result.ran_at, result.plan.as_ref()))
    }

    fn set_plan_agenda(&mut self, title: &str, agenda: Vec<Event>) -> bool {
        self.app.action_results.set_agenda(title, agenda)
    }

    fn plans_changed(&mut self) {
        self.app.save_action_results();
        self.app.render_plan();
    }

    fn chat_in_use(&self) -> bool {
        !self.app.runs.is_empty() || self.app.chat.is_visible()
    }

    fn run_quick_action(&mut self, index: usize) {
        self.app.run_action(index);
    }

    fn tasks(&mut self) -> &mut TaskList {
        &mut self.app.tasks
    }

    fn tasks_changed(&mut self) {
        self.app.tasks_changed();
    }

    fn set_tasks_status(&mut self, status: Option<&str>) {
        let title = match status {
            Some(status) => format!("Tasks · {status}"),
            None => "Tasks".to_string(),
        };
        self.app.chat.set_tasks_title(&title);
    }

    fn wear(&mut self, coat: Coat) {
        self.app.wear(coat);
    }

    fn resize_chat(&mut self, size: (f64, f64)) {
        self.app.chat.set_size(size);
    }

    fn show_news(&mut self, shown: bool) {
        self.app.activity.news_shown = shown;
    }

    fn settings_changed(&mut self) {
        self.app.settings_changed = true;
    }

    fn record(&mut self, note: Note) {
        self.app.notify(note);
    }

    /// None while taking snapshots: they show pages, and never run Claude to
    /// act or change the user's data.
    fn claude(&self) -> Option<&ClaudeEnv> {
        if self.app.snapshot.is_some() {
            return None;
        }
        self.app.claude.as_ref()?.as_ref().ok()
    }

    fn claude_or_why(&self) -> Result<&ClaudeEnv, String> {
        if self.app.snapshot.is_some() {
            return Err("Snapshots do not run Claude.".to_string());
        }
        match &self.app.claude {
            Some(Ok(env)) => Ok(env),
            Some(Err(error)) => Err(format!("Claude is not available: {error}")),
            None => Err("Claude is still starting; try again in a moment.".to_string()),
        }
    }

    fn set_badge(&mut self, page: &'static str, count: usize) {
        self.app.chat.set_badge(page, count);
    }

    fn is_idle(&self) -> bool {
        !self.others_busy && self.app.is_idle()
    }

    /// Done once the features are back in the app, so each can save first.
    fn quit(&mut self) {
        self.app.quitting = true;
    }
}

impl App {
    pub(super) fn poll_features(&mut self) {
        // Out of `self` while they run, so each can be handed the app. So the
        // app code a `Host` method reaches (e.g. `run_quick_action` starting a
        // request) must not use `self.features`: it is empty meanwhile.
        let mut features = std::mem::take(&mut self.features);
        for index in 0..features.len() {
            let (before, rest) = features.split_at_mut(index);
            let Some((feature, after)) = rest.split_first_mut() else {
                continue;
            };
            let others_busy = before.iter().chain(after.iter()).any(|f| f.is_busy());
            feature.poll(&mut AppHost {
                app: self,
                others_busy,
            });
        }
        self.features = features;
        self.after_features();
    }

    /// What features asked for while they were out of `self`.
    fn after_features(&mut self) {
        if std::mem::take(&mut self.menu_changed) {
            self.fill_cat_menu();
        }
        // Not under the user's typing or click: tried again each tick until done.
        if self.settings_changed && !self.chat.settings().in_use() {
            self.settings_changed = false;
            self.render_settings();
        }
        if std::mem::take(&mut self.quitting) {
            self.quit();
        }
    }

    /// Fills the cat's menu with the features' items as they are now.
    pub(super) fn fill_cat_menu(&self) {
        let entries = |section| {
            self.features
                .iter()
                .filter(|f| f.menu_section() == section)
                .flat_map(|f| f.menu())
                .collect()
        };
        menus::fill_cat_menu(
            self.mtm,
            &self.cat_menu,
            &self.controller,
            &self.actions,
            entries(MenuSection::Cat),
            entries(MenuSection::App),
            self.coat.is_pig(),
        );
    }

    /// Draws the Settings page with the features' sections as they are now.
    pub(super) fn render_settings(&self) {
        let sections = self.features.iter().flat_map(|f| f.settings()).collect();
        self.chat.settings().render(sections);
    }

    /// Whether a feature is doing something the user waits on.
    pub(super) fn features_thinking(&self) -> bool {
        self.features.iter().any(|f| f.thinking())
    }

    /// Dresses the cat in `coat`; the hairball item becomes Poop for Dédé.
    fn wear(&mut self, coat: Coat) {
        self.coat = coat;
        self.cat.set_pig(coat.is_pig());
        self.chat.set_coat(coat);
        // Redraw now, even if the cat holds still.
        self.last_look = None;
        self.menu_changed = true;
    }

    fn flag_unread(&mut self, chat: &ChatId) {
        if !(self.chat.is_visible() && self.conversations.active() == chat) {
            self.unread = true;
            self.unread_chat = chat.clone();
        }
    }

    /// Whether a feature takes the command `text` typed in conversation `chat`.
    pub(super) fn feature_command(&mut self, text: &str, chat: &ChatId) -> bool {
        self.features.iter_mut().any(|f| f.command(text, chat))
    }

    /// What the features add to the prompt of a message typed in the chat.
    pub(super) fn feature_instructions(&self, now: u64) -> Vec<String> {
        self.features
            .iter()
            .filter_map(|f| f.chat_instruction(now))
            .collect()
    }

    /// Reply `text` without the blocks the features read in it.
    pub(super) fn features_read_reply(&mut self, chat: &ChatId, text: &str, done: bool) -> String {
        self.features
            .iter_mut()
            .fold(text.to_string(), |text, f| f.read_reply(chat, &text, done))
    }

    /// Opens the chat bubble on `page`, letting its feature refresh it first.
    pub(super) fn open_page(&mut self, page: Page) {
        if !self.chat.is_visible() {
            self.open_window();
        }
        if let Page::Feature(id) = page {
            self.features
                .iter_mut()
                .filter(|f| f.pages().iter().any(|spec| spec.id == id))
                .for_each(|f| f.page_shown(id));
        }
        if page == Page::Settings && !self.chat.settings().in_use() {
            self.settings_changed = false;
            self.render_settings();
        }
        self.chat.show_page(page);
    }

    /// Runs `each` on the features, with the app as their host, until one
    /// returns true; whether one did. As in `poll_features`, they are out of
    /// `self` meanwhile.
    fn with_features(
        &mut self,
        mut each: impl FnMut(&mut dyn Feature, &mut dyn Host) -> bool,
    ) -> bool {
        let mut features = std::mem::take(&mut self.features);
        let done = features.iter_mut().any(|feature| {
            let mut host = AppHost {
                app: self,
                others_busy: false,
            };
            each(feature.as_mut(), &mut host)
        });
        self.features = features;
        self.after_features();
        done
    }

    /// Whether a feature shows what Activity `link` is about.
    pub(super) fn features_open_link(&mut self, link: &Link) -> bool {
        self.with_features(|feature, host| feature.open_link(link, host))
    }

    /// Whether a feature did something with Esc on its page.
    pub(super) fn features_escape(&mut self) -> bool {
        self.with_features(|feature, host| feature.escape(host))
    }

    pub(super) fn features_quit(&mut self) {
        self.features.iter_mut().for_each(|feature| feature.quit());
    }

    /// The snapshot steps the features add, in their order.
    pub(super) fn feature_snapshot_steps(&self) -> Vec<&'static str> {
        self.features
            .iter()
            .flat_map(|feature| feature.snapshot_steps())
            .collect()
    }

    /// Shows snapshot step `step`, through the feature that adds it.
    pub(super) fn show_feature_snapshot(&mut self, step: &'static str) {
        self.with_features(|feature, host| {
            let owns = feature.snapshot_steps().contains(&step);
            if owns {
                feature.show_snapshot(step, host);
            }
            owns
        });
    }

    /// Nothing outside the features that a restart would lose.
    fn is_idle(&self) -> bool {
        self.runs.is_empty()
            && !self.chat.is_visible()
            && !self.rsvp.is_running()
            && !self.agents.is_running()
    }
}

/// Builds each feature's page into the chat bubble, whose sidebar already
/// lists them (see `ChatWindow::new`).
pub(super) fn attach_pages(
    mtm: MainThreadMarker,
    controller: &Retained<Controller>,
    features: &mut [Box<dyn Feature>],
    chat: &crate::ui::chat_window::ChatWindow,
) {
    let kit = PageKit {
        mtm,
        frame: chat.page_frame(),
        controller,
    };
    for feature in features {
        for spec in feature.pages() {
            if let Some(page) = feature.build_page(spec.id, &kit) {
                chat.add_page(spec.id, page);
            }
        }
    }
}
