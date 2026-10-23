//! Ties the cat, the chat bubble, the hotkey and Claude runs together.

mod activity;
pub(crate) mod agents;
mod calendar;
pub(crate) mod feature;
pub(crate) mod mailbox;
mod plan;
mod quick_actions;
mod requests;
mod rsvp;
mod snapshot;
mod task_actions;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use meow::action_results::ActionResults;
use meow::actions::{self, QuickAction};
use meow::behavior::{Bounds, Cat, Inputs, PropView, Rng};
use meow::claude::ClaudeEnv;
use meow::conversations::{ChatId, Conversation, Conversations};
use meow::settings::{self, Store};
use meow::source::Icons;
use meow::sprites::{Coat, Facing, Look};
use meow::tasks::{self, TaskList};
use meow::transcript::Transcript;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEvent, NSMenu, NSScreen};
use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSTimer};

use crate::ui::cat_window::{self, CatWindow};
use crate::ui::chat_window::ChatWindow;
use crate::ui::controller::Controller;
use crate::ui::events::{self, UiEvent};
use crate::ui::menus;
use crate::ui::news_bubble::NewsBubbleWindow;
use crate::ui::pointer;
use crate::ui::screens;
use crate::ui::sidebar::{Page, Selection};
use crate::ui::theme;

use requests::ActiveRun;

const TICK_SECONDS: f64 = 1.0 / 30.0;
const MAX_STEP_SECONDS: f64 = 0.1;
pub const HOTKEY_LABEL: &str = "⌥⇧Space";

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct App {
    mtm: MainThreadMarker,
    controller: Retained<Controller>,
    cat_window: CatWindow,
    /// The part of the cat over the next screen, while it crosses over.
    cat_overflow: CatWindow,
    /// The cat's bowl, or the mouse it chases.
    prop_window: CatWindow,
    last_prop: Option<PropView>,
    /// The hairball the cat left on the floor, until it is wiped away.
    hairball_window: CatWindow,
    last_hairball: Option<PropView>,
    /// The screen height of the ground the hairball lies on: it stays there
    /// when the cat moves to another screen.
    hairball_ground: f64,
    /// Where unread news comes from, above the cat's head in place of its `!`.
    news_bubble: NewsBubbleWindow,
    /// The icons and side the bubble last showed them on.
    last_news: Option<(Icons, Facing)>,
    /// Where the cat holds the pointer still, while it has it caught.
    held_pointer: Option<NSPoint>,
    chat: ChatWindow,
    conversations: Conversations,
    tasks: TaskList,
    /// A task just ticked on the tasks page, waiting for "draft reply" or
    /// "no reply" until this deadline, after which it is done with no reply.
    pending_done: Option<(String, Instant)>,
    /// Where tasks are saved; `None` when the file could not be read, so it is never overwritten.
    tasks_path: Option<PathBuf>,
    action_results: ActionResults,
    /// Where action results are saved; `None` when the file could not be read.
    results_path: Option<PathBuf>,
    /// The settings file, shared with the features.
    store: Store,
    /// The cat's right-click menu.
    cat_menu: Retained<NSMenu>,
    /// A feature changed its menu items: the cat's menu is filled again.
    menu_changed: bool,
    /// A feature asked to quit, done once the features are back (see
    /// `feature::after_features`).
    quitting: bool,
    /// What the cat wears, as the appearance feature says.
    coat: Coat,
    cat: Cat,
    /// The screen height of the ground the cat's height is measured from:
    /// the bottom of the screen it is on.
    ground: f64,
    rng: Rng,
    last_look: Option<Look>,
    last_tick: Instant,
    claude: Option<Result<ClaudeEnv, String>>,
    discovery: Option<Receiver<Result<ClaudeEnv, String>>>,
    /// Requests in flight, one at most per conversation.
    runs: HashMap<ChatId, ActiveRun>,
    actions: Vec<QuickAction>,
    /// The calendar tools of the user's Claude Code, for Slack's events, the
    /// agenda and invitation answers.
    calendar: calendar::CalendarState,
    /// Invitation answers on their way to the calendar.
    rsvp: rsvp::RsvpState,
    /// What happened in the background, for the Activity page.
    activity: activity::ActivityState,
    /// Coding agents started in Superset from the chat or Slack.
    agents: agents::AgentsState,
    /// The features moved to `src/features/` (see ARCHITECTURE.md).
    features: Vec<Box<dyn feature::Feature>>,
    /// A reply arrived out of sight; the cat shows `!` until it is seen.
    unread: bool,
    /// The conversation whose reply is waiting.
    unread_chat: ChatId,
    /// Debug snapshots of each page, when `MEOW_SNAPSHOT_DIR` is set.
    snapshot: Option<snapshot::Snapshot>,
    _hotkeys: Option<GlobalHotKeyManager>,
}

pub fn start(mtm: MainThreadMarker) {
    // Before any window is built, so every label gets the pixel font.
    let fonts_error = theme::register_fonts().err();
    // Before anything is loaded, so data saved under the old name is found.
    let migration_errors = std::env::var_os("HOME")
        .map(|home| meow::legacy::migrate_data(std::path::Path::new(&home)))
        .unwrap_or_default();
    let controller = Controller::new(mtm);
    let (actions, actions_error) = load_actions();
    let (mut store, settings_error) = load_settings();
    let mut features = crate::features::all(&mut store);
    menus::install_main_menu(mtm, &controller);
    let cat_menu = menus::cat_menu(mtm);
    let cat_window = CatWindow::new(mtm, &cat_menu);
    let prop_window = CatWindow::prop(mtm);
    let pages: Vec<_> = features.iter().flat_map(|f| f.pages()).collect();
    let chat = ChatWindow::new(mtm, &controller, &actions, &pages);
    feature::attach_pages(mtm, &controller, &mut features, &chat);

    let mut main = Transcript::default();
    let (hotkeys, hotkey_error) = match register_hotkey() {
        Ok(manager) => (Some(manager), None),
        Err(e) => (None, Some(format!("Global shortcut unavailable: {e}"))),
    };
    let actions_error = actions_error.map(|e| format!("Using the default quick actions. {e}"));
    main.info(&format!(
        "Click the cat or press {HOTKEY_LABEL} to talk. Right-click for options."
    ));
    let (action_results, results_path, results_error) =
        quick_actions::load_action_results(&actions);
    let (activity_state, activity_error) = activity::ActivityState::load();
    chat.render_activity(&activity_state.log, now_secs());
    crate::ui::notify::request_permission();
    let startup_errors = [
        &activity_error,
        &fonts_error,
        &actions_error,
        &hotkey_error,
        &results_error,
        &settings_error,
    ];
    let feature_errors: Vec<_> = features.iter().filter_map(|f| f.startup_error()).collect();
    for error in startup_errors
        .into_iter()
        .flatten()
        .chain(&feature_errors)
        .chain(&migration_errors)
    {
        eprintln!("meow: {error}");
        main.error(error);
    }
    let (task_list, tasks_path) = load_tasks(&mut main);
    chat.render_tasks(&task_list, now_secs());
    chat.chat().render(&main, true);
    let mut conversations = Conversations::default();
    conversations.get_mut(&ChatId::Main).transcript = main;

    let (discovered, discovery) = mpsc::channel();
    thread::spawn(move || {
        let _ = discovered.send(ClaudeEnv::discover());
    });

    let (start_x, ground) = NSScreen::screens(mtm)
        .firstObject()
        .map(|screen| {
            let (bounds, ground) = screen_bounds(&screen);
            ((bounds.min_x + bounds.max_x) / 2.0, ground)
        })
        .unwrap_or((0.0, 0.0));
    let app = App {
        mtm,
        controller,
        cat_window,
        cat_overflow: CatWindow::overflow(mtm),
        prop_window,
        last_prop: None,
        hairball_window: CatWindow::hairball(mtm),
        last_hairball: None,
        hairball_ground: ground,
        news_bubble: NewsBubbleWindow::new(mtm),
        last_news: None,
        held_pointer: None,
        chat,
        conversations,
        tasks: task_list,
        pending_done: None,
        tasks_path,
        action_results,
        results_path,
        store,
        cat_menu,
        menu_changed: false,
        quitting: false,
        // Dressed by the appearance feature at the first tick, before drawing.
        coat: Coat::default(),
        cat: Cat::new(start_x),
        ground,
        rng: Rng::new(seed()),
        last_look: None,
        last_tick: Instant::now(),
        unread: startup_errors.iter().any(|e| e.is_some())
            || !feature_errors.is_empty()
            || !migration_errors.is_empty(),
        unread_chat: ChatId::Main,
        claude: None,
        discovery: Some(discovery),
        runs: HashMap::new(),
        actions,
        calendar: calendar::CalendarState::default(),
        rsvp: rsvp::RsvpState::default(),
        activity: activity_state,
        agents: agents::AgentsState::default(),
        features,
        snapshot: snapshot::Snapshot::from_env(),
        _hotkeys: hotkeys,
    };
    app.fill_cat_menu();
    APP.with(|cell| cell.replace(Some(app)));
    schedule_ticks();
}

fn schedule_ticks() {
    let block = RcBlock::new(|_timer: NonNull<NSTimer>| {
        APP.with(|cell| {
            if let Ok(mut app) = cell.try_borrow_mut()
                && let Some(app) = app.as_mut()
            {
                app.tick();
            }
        });
    });
    // SAFETY: the block only touches main-thread state and the timer fires on the main run
    // loop. Common modes keep the cat moving while a menu is open or a window is resized.
    unsafe {
        let timer = NSTimer::timerWithTimeInterval_repeats_block(TICK_SECONDS, true, &block);
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
    }
}

/// The settings file, or why it could not be read (then nothing is saved).
fn load_settings() -> (Store, Option<String>) {
    let Some(path) = settings::default_path() else {
        return (Store::unsaved(), None);
    };
    match Store::load(&path) {
        Ok(store) => (store, None),
        Err(error) => (Store::unsaved(), Some(unsaved(&error))),
    }
}

/// Why settings could not be read, and what that means.
pub(crate) fn unsaved(error: &str) -> String {
    format!("{error}. Settings changes won't be saved.")
}

/// Quick actions from the config file, or the defaults plus the reason it could not be used.
fn load_actions() -> (Vec<QuickAction>, Option<String>) {
    let Some(path) = actions::config_path() else {
        return (actions::defaults(), None);
    };
    match actions::load(&path) {
        Ok(list) => (list, None),
        Err(error) => (actions::defaults(), Some(error)),
    }
}

/// Saved tasks, minus those done over a day ago. A file that cannot be read is
/// reported and left untouched.
fn load_tasks(transcript: &mut Transcript) -> (TaskList, Option<PathBuf>) {
    let Some(path) = tasks::default_path() else {
        return (TaskList::default(), None);
    };
    match TaskList::load(&path) {
        Ok(mut list) => {
            list.prune(now_secs());
            (list, Some(path))
        }
        Err(error) => {
            eprintln!("meow: {error}");
            transcript.error(&format!("{error}. Task changes won't be saved."));
            (TaskList::default(), None)
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn register_hotkey() -> Result<GlobalHotKeyManager, String> {
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    manager
        .register(HotKey::new(
            Some(Modifiers::ALT | Modifiers::SHIFT),
            Code::Space,
        ))
        .map_err(|e| e.to_string())?;
    Ok(manager)
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

/// Where the cat window may go on `screen`, and the ground's screen height.
fn screen_bounds(screen: &NSScreen) -> (Bounds, f64) {
    let visible = screen.visibleFrame();
    let bounds = Bounds::new(
        visible.origin.x,
        visible.origin.x + visible.size.width - cat_window::WIDTH,
        visible.size.height - cat_window::HEIGHT,
    );
    (bounds, visible.origin.y)
}

impl App {
    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .as_secs_f64()
            .min(MAX_STEP_SECONDS);
        self.last_tick = now;

        for event in events::drain() {
            self.handle(event);
        }
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed {
                self.toggle_chat();
            }
        }
        self.poll_discovery();
        self.poll_claude();
        self.poll_calendar_probe();
        self.poll_pending_done();
        self.poll_rsvp();
        self.poll_agents();
        self.poll_snapshot();
        self.poll_features();
        self.animate(dt);
    }

    fn inputs(&self) -> Inputs {
        Inputs {
            // Looking at a photo of the user's cat shows too: it takes a while.
            busy: !self.runs.is_empty() || self.features_thinking(),
            chat_open: self.chat.is_visible(),
            unread: self.unread,
            news: self.activity.log.unread_count() > 0,
        }
    }

    /// Where the cat may go on the screen it is on, that screen's ground and
    /// its frame. Carried or thrown to another screen, the cat lives there from
    /// then on: its height is measured again from the new ground.
    fn cat_screen(&mut self) -> Option<(Bounds, f64, NSRect)> {
        let center = NSPoint::new(
            self.cat.x + cat_window::WIDTH / 2.0,
            self.ground + self.cat.y + cat_window::HEIGHT / 2.0,
        );
        let screen = screens::showing(self.mtm, center)?;
        let (bounds, ground) = screen_bounds(&screen);
        self.cat.raise_ground(ground - self.ground);
        self.ground = ground;
        let open = screens::open_edges(self.mtm, &screen, center);
        Some((Bounds { open, ..bounds }, ground, screen.frame()))
    }

    fn animate(&mut self, dt: f64) {
        let Some((bounds, ground, frame)) = self.cat_screen() else {
            return;
        };
        let inputs = self.inputs();
        let pointer = NSEvent::mouseLocation();
        self.cat.see_pointer(pointer.x, pointer.y - ground);
        self.cat.update(dt, bounds, inputs, &mut self.rng);
        self.show_cat(frame, NSPoint::new(self.cat.x, ground + self.cat.y));
        self.show_prop(ground);
        self.show_hairball(ground);
        self.hold_pointer(ground);

        let look = self.cat.look(inputs);
        let origin = NSPoint::new(self.cat.x, ground + self.cat.y);
        let look = self.show_news(look, origin, frame);
        if self.last_look != Some(look) {
            let coat = self.coat;
            self.cat_overflow.view.show_look(look, coat);
            self.cat_window.view.show_look(look, coat);
            self.last_look = Some(look);
        }
    }

    /// Shows the cat at `origin` on its screen (with `frame`) and, while it
    /// crosses over to the next screen, the part of it over there in a second
    /// window: each stays whole on its own screen.
    fn show_cat(&mut self, frame: NSRect, origin: NSPoint) {
        self.cat_window.show_on(frame, origin);
        let rect = NSRect::new(origin, NSSize::new(cat_window::WIDTH, cat_window::HEIGHT));
        let overflow = &self.cat_overflow.panel;
        match screens::next_under(self.mtm, rect, frame) {
            Some(next) => {
                self.cat_overflow.show_on(next.frame(), origin);
                if !overflow.isVisible() {
                    overflow.orderFrontRegardless();
                }
            }
            None if overflow.isVisible() => overflow.orderOut(None),
            None => {}
        }
    }

    /// Pins the pointer under the cat's paw while it has it caught.
    fn hold_pointer(&mut self, ground: f64) {
        let held = self
            .cat
            .caught_pointer()
            .map(|(x, y)| NSPoint::new(x.round(), (ground + y).round()));
        if held == self.held_pointer {
            return;
        }
        match held {
            Some(at) => pointer::hold(self.mtm, at),
            None => pointer::release(),
        }
        self.held_pointer = held;
    }

    fn show_prop(&mut self, ground: f64) {
        let prop = self.cat.prop();
        if prop == self.last_prop {
            return;
        }
        match prop {
            Some(view) => {
                self.prop_window.move_to(view.x as f64, ground);
                self.prop_window.view.show_prop(view.prop, view.facing);
                if self.last_prop.is_none() {
                    self.prop_window.panel.orderFrontRegardless();
                    // The cat's head goes over the bowl's rim.
                    self.cat_window.panel.orderFrontRegardless();
                }
            }
            None => self.prop_window.panel.orderOut(None),
        }
        self.last_prop = prop;
    }

    /// Shows the hairball where it landed, on the ground of the screen the cat
    /// was on then.
    fn show_hairball(&mut self, ground: f64) {
        let hairball = self.cat.hairball();
        if hairball == self.last_hairball {
            return;
        }
        match hairball {
            Some(view) => {
                if self.last_hairball.is_none() {
                    self.hairball_ground = ground;
                }
                self.hairball_window
                    .move_to(view.x as f64, self.hairball_ground);
                self.hairball_window.view.show_prop(view.prop, view.facing);
                if self.last_hairball.is_none() {
                    self.hairball_window.panel.orderFrontRegardless();
                    // The cat walks in front of it.
                    self.cat_window.panel.orderFrontRegardless();
                }
            }
            None => self.hairball_window.panel.orderOut(None),
        }
        self.last_hairball = hairball;
    }

    /// Feeds the cat, or lets a mouse in, from its menu.
    fn play(&mut self, event: UiEvent) {
        let Some((bounds, ..)) = self.cat_screen() else {
            return;
        };
        match event {
            UiEvent::Feed => self.cat.feed(bounds),
            _ => self.cat.release_mouse(bounds, &mut self.rng),
        };
    }

    fn handle(&mut self, event: UiEvent) {
        match event {
            UiEvent::CatClicked | UiEvent::ToggleChat => self.toggle_chat(),
            UiEvent::Grab => self.cat.grab(),
            UiEvent::DragTo { x, y } => self.cat.drag_to(x, y - self.ground),
            UiEvent::Drop => self.cat.release(),
            UiEvent::Stroke(distance) => self.cat.stroke(distance),
            UiEvent::Feed | UiEvent::ReleaseMouse => self.play(event),
            UiEvent::CoughUp => {
                self.cat.cough_up();
            }
            UiEvent::WipeHairball(distance) => self.cat.wipe_hairball(distance),
            UiEvent::TapHairball => self.cat.tap_hairball(),
            UiEvent::QuickAction(index) => self.quick_action(index),
            UiEvent::RunAction => self.rerun_shown_action(),
            UiEvent::ShowChat => self.show_conversation(ChatId::Main),
            UiEvent::ShowTasks => {
                if !self.chat.is_visible() {
                    self.open_window();
                }
                self.chat.show_page(Page::Tasks);
            }
            UiEvent::AddTask => self.add_task(),
            UiEvent::ToggleTask(row) => self.toggle_task(row),
            UiEvent::DraftDoneReply(row) => self.draft_done_reply(row),
            UiEvent::NoDoneReply(row) => self.no_done_reply(row),
            UiEvent::ToggleCompleted => {
                self.chat.tasks().toggle_completed();
                self.chat.render_tasks(&self.tasks, now_secs());
            }
            UiEvent::OpenTask(row) => self.open_task(row),
            UiEvent::ChatAboutTask(row) => self.chat_about_task(row),
            UiEvent::ShowActivity => self.show_activity(),
            UiEvent::OpenActivity(row) => self.open_activity(row),
            UiEvent::DismissActivity(row) => self.dismiss_activity(row),
            UiEvent::ClearActivity => self.clear_activity(),
            UiEvent::ShowPage(page) => self.open_page(page),
            UiEvent::AskPlan => self.ask_about_plan(),
            UiEvent::TogglePlanItem(row) => self.toggle_plan_item(row),
            UiEvent::OpenPlanItem(row) => self.open_plan_item(row),
            UiEvent::OpenPlanLink(index) => self.open_plan_link(index),
            UiEvent::AcceptPlanEvent(index) => {
                self.answer_plan_event(index, meow::plan::Rsvp::Accepted)
            }
            UiEvent::DeclinePlanEvent(index) => {
                self.answer_plan_event(index, meow::plan::Rsvp::Declined)
            }
            UiEvent::ToggleSteps(turn) => self.toggle_steps(turn),
            UiEvent::Submit => self.submit(),
            // Esc stops the request of the conversation on screen, and only
            // there: other pages and conversations keep theirs running.
            UiEvent::Escape
                if self.chat.page() == Page::Chat
                    && self.is_running(self.conversations.active()) =>
            {
                let id = self.conversations.active().clone();
                self.cancel_run(&id)
            }
            UiEvent::Escape => {
                if !self.features_escape() {
                    self.chat.hide();
                }
            }
            // Checked on the next tick: focus may already be back (e.g. a sheet closed).
            UiEvent::FocusLost if !self.chat.is_key() => self.chat.hide(),
            UiEvent::FocusLost => {}
            UiEvent::NewConversation => self.new_conversation(),
            UiEvent::Quit => self.quit(),
        }
    }

    fn quit(&mut self) {
        pointer::release();
        self.features_quit();
        NSApplication::sharedApplication(self.mtm).terminate(None);
    }

    fn toggle_chat(&mut self) {
        if self.chat.is_visible() && self.chat.is_key() {
            self.chat.hide();
            return;
        }
        if self.unread {
            // A reply is waiting: show it rather than whatever was open.
            self.unread = false;
            self.show_conversation(self.unread_chat.clone());
        } else if self.activity.log.unread_count() > 0 {
            // The bubble over the cat showed news: show where it is.
            return self.show_activity();
        }
        self.open_window();
    }

    fn open_window(&self) {
        NSApplication::sharedApplication(self.mtm).activate();
        self.chat.show_above(self.cat_frame(), self.mtm);
    }

    fn cat_frame(&self) -> NSRect {
        self.cat_window.frame()
    }

    /// Shows conversation `id`, opening it the first time: a plan action's
    /// plan on the plan page, anything else (or a run in progress) on the chat page.
    fn show_conversation(&mut self, id: ChatId) {
        self.open_conversation(id, true);
    }

    fn open_conversation(&mut self, id: ChatId, prefer_plan: bool) {
        let fresh = self.fresh_conversation(&id);
        self.conversations.open(id.clone(), || fresh);
        self.update_chat_title();
        let running = self.is_running(&id);
        let has_plan = match id {
            ChatId::Action(index) => self.plan_of(index).is_some(),
            ChatId::Main | ChatId::Task(_) => false,
        };
        if prefer_plan && has_plan && !running {
            self.render_plan();
            self.chat.show_page(Page::Plan);
            return;
        }
        self.chat.show_page(Page::Chat);
        self.chat
            .chat()
            .render(&self.conversations.current().transcript, true);
    }

    /// How conversation `id` starts the first time it is shown.
    fn fresh_conversation(&self, id: &ChatId) -> Conversation {
        match id {
            ChatId::Main => Conversation::default(),
            ChatId::Task(task) => {
                Conversation::resuming(self.tasks.get(task).and_then(|t| t.session.clone()))
            }
            ChatId::Action(index) => self.saved_action_conversation(*index),
        }
    }

    /// Header and sidebar highlight for the conversation on the chat page.
    fn update_chat_title(&self) {
        let (title, selection, can_run) = match self.conversations.active() {
            ChatId::Main => ("Chat".to_string(), Selection::Chat, false),
            ChatId::Task(task) => {
                let title = match self.tasks.get(task) {
                    Some(task) => format!("{} · {}", task.source, task.title),
                    None => "Task".to_string(),
                };
                (title, Selection::Tasks, false)
            }
            ChatId::Action(index) => (self.action_title(*index), Selection::Action(*index), true),
        };
        self.chat.set_chat_title(&title, selection, can_run);
    }

    fn poll_discovery(&mut self) {
        let Some(discovery) = &self.discovery else {
            return;
        };
        let result = match discovery.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("Could not look for the claude command.".to_string())
            }
        };
        if let Err(error) = &result {
            eprintln!("meow: {error}");
            self.error_in(&ChatId::Main, error);
            self.unread = true;
            self.unread_chat = ChatId::Main;
        }
        self.claude = Some(result);
        self.discovery = None;
        self.discover_superset();
    }

    /// Redraws conversation `id` if it is the one on screen.
    fn refresh(&self, id: &ChatId, scroll_to_end: bool) {
        if self.conversations.active() == id {
            self.chat
                .chat()
                .render(&self.conversations.current().transcript, scroll_to_end);
        }
    }

    fn transcript(&mut self, id: &ChatId) -> &mut Transcript {
        &mut self.conversations.get_mut(id).transcript
    }

    fn info_in(&mut self, id: &ChatId, text: &str) {
        self.transcript(id).info(text);
        self.refresh(id, true);
    }

    fn error_in(&mut self, id: &ChatId, text: &str) {
        self.transcript(id).error(text);
        self.refresh(id, true);
    }

    /// A note in the conversation on screen.
    fn info(&mut self, text: &str) {
        let id = self.conversations.active().clone();
        self.info_in(&id, text);
    }

    fn toggle_steps(&mut self, turn: usize) {
        let id = self.conversations.active().clone();
        self.transcript(&id).toggle(turn);
        self.refresh(&id, false);
    }
}
