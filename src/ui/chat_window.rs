//! The main floating window, drawn as the cat's speech bubble: a sidebar on
//! the left, and the chat, task or plan page.

use std::cell::{Cell, RefCell};

use meow::actions::QuickAction;
use meow::activity::ActivityLog;
use meow::autopilot::Jobs;
use meow::routines::Routines;
use meow::tasks::TaskList;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions,
    NSBitmapImageFileType, NSButton, NSColor, NSLineBreakMode, NSStatusWindowLevel,
    NSTextAlignment, NSTextField, NSView, NSWindowCollectionBehavior,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use super::activity_page::ActivityPage;
use super::autopilot_page::AutopilotPage;
use super::bubble::{self, BubblePanel, BubbleView};
use super::chat_page::ChatPage;
use super::controller::Controller;
use super::plan_page::PlanPage;
use super::rich_text::{self, Attributes};
use super::routines_page::RoutinesPage;
use super::screens;
use super::sidebar::{self, Page, Selection, Sidebar};
use super::slack_page::SlackPage;
use super::task_page::TaskPage;
use super::theme;
use super::widgets;

const WIDTH: f64 = 620.0;
const HEIGHT: f64 = 470.0;
const MIN_SIZE: NSSize = NSSize::new(480.0, 300.0);
const CLOSE_SIZE: f64 = 26.0;
const HEADER_INSET: f64 = 10.0;
const RUN_WIDTH: f64 = 70.0;
/// Least space between the page title and its status.
const SUBTITLE_GAP: f64 = 16.0;

pub struct ChatWindow {
    panel: Retained<BubblePanel>,
    /// The size picked in the settings, while the bubble is shrunk to fit a
    /// smaller screen; restored the next time it opens.
    unfitted: Cell<Option<NSSize>>,
    bubble: Retained<BubbleView>,
    sidebar: Sidebar,
    chat: ChatPage,
    tasks: TaskPage,
    plan: PlanPage,
    slack: SlackPage,
    activity: ActivityPage,
    routines: RoutinesPage,
    autopilot: AutopilotPage,
    page: Cell<Page>,
    /// Top line of the page: "Chat", "Tasks", or the task being discussed.
    header: Retained<NSTextField>,
    /// Right end of the top line: "6 open · checked at 09:40", "ran 7 h ago".
    subtitle: Retained<NSTextField>,
    open_tasks: Cell<usize>,
    /// Re-runs the quick action shown on the chat page.
    run_button: Retained<NSButton>,
    chat_title: RefCell<String>,
    /// "Tasks", with the background check's status.
    tasks_title: RefCell<String>,
    chat_selection: Cell<Selection>,
    can_run: Cell<bool>,
}

impl ChatWindow {
    pub fn new(
        mtm: MainThreadMarker,
        controller: &Retained<Controller>,
        actions: &[QuickAction],
    ) -> Self {
        let panel = make_panel(mtm);
        let bubble = BubbleView::new(mtm, rect(0.0, 0.0, WIDTH, HEIGHT));
        bubble.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        panel.setContentView(Some(&bubble));
        // Tells the controller when the bubble loses focus, so it can close.
        panel.setDelegate(Some(ProtocolObject::from_ref(&**controller)));

        let inner = bubble.content_rect();
        let sidebar = Sidebar::new(mtm, controller, actions, inner);
        bubble.addSubview(&sidebar.view);
        bubble.set_sidebar_right(inner.origin.x + sidebar::WIDTH);

        // Pages run on to the outline on the right and at the bottom, so their
        // bars and strips can reach it; each page keeps its own margins.
        let page_frame = rect(
            inner.origin.x + sidebar::WIDTH,
            inner.origin.y + CLOSE_SIZE,
            inner.size.width - sidebar::WIDTH + bubble::PADDING,
            inner.size.height - CLOSE_SIZE + bubble::PADDING,
        );
        let chat = ChatPage::new(mtm, controller, page_frame);
        let tasks = TaskPage::new(mtm, controller, page_frame);
        let plan = PlanPage::new(mtm, controller, page_frame);
        let slack = SlackPage::new(mtm, controller, page_frame);
        let activity = ActivityPage::new(mtm, controller, page_frame);
        let routines = RoutinesPage::new(mtm, controller, page_frame);
        let autopilot = AutopilotPage::new(mtm, controller, page_frame);
        bubble.addSubview(&chat.view);
        bubble.addSubview(&tasks.view);
        bubble.addSubview(&plan.view);
        bubble.addSubview(&slack.view);
        bubble.addSubview(&activity.view);
        bubble.addSubview(&routines.view);
        bubble.addSubview(&autopilot.view);
        bubble.addSubview(&close_button(mtm, controller, inner));
        let header = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        header.setFrame(rect(
            inner.origin.x + sidebar::WIDTH + HEADER_INSET,
            inner.origin.y + 2.0,
            inner.size.width - sidebar::WIDTH - CLOSE_SIZE - RUN_WIDTH - 2.0 * HEADER_INSET,
            CLOSE_SIZE - 2.0,
        ));
        header.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        header.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        bubble.addSubview(&header);
        // Same row as the title, pinned to the top-right corner (see `layout_subtitle`).
        let subtitle = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        subtitle.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        subtitle.setAlignment(NSTextAlignment::Right);
        subtitle.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
        bubble.addSubview(&subtitle);

        let target: &AnyObject = controller;
        let run_button = widgets::text_button(
            mtm,
            "Run",
            widgets::ButtonStyle::Primary,
            target,
            sel!(runAction:),
        );
        run_button.setToolTip(Some(&NSString::from_str("Run this quick action again")));
        run_button.setFrame(rect(
            inner.origin.x + inner.size.width - CLOSE_SIZE - RUN_WIDTH,
            inner.origin.y,
            RUN_WIDTH,
            CLOSE_SIZE,
        ));
        // Flipped bubble: a flexible bottom margin keeps it in the top row.
        run_button.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        bubble.addSubview(&run_button);

        let window = Self {
            panel,
            unfitted: Cell::new(None),
            bubble,
            sidebar,
            chat,
            tasks,
            plan,
            slack,
            activity,
            routines,
            autopilot,
            page: Cell::new(Page::Chat),
            header,
            subtitle,
            open_tasks: Cell::new(0),
            run_button,
            chat_title: RefCell::new("Chat".to_string()),
            tasks_title: RefCell::new("Tasks".to_string()),
            chat_selection: Cell::new(Selection::Chat),
            can_run: Cell::new(false),
        };
        window.show_page(Page::Chat);
        window
    }

    pub fn chat(&self) -> &ChatPage {
        &self.chat
    }

    pub fn tasks(&self) -> &TaskPage {
        &self.tasks
    }

    pub fn plan(&self) -> &PlanPage {
        &self.plan
    }

    pub fn slack(&self) -> &SlackPage {
        &self.slack
    }

    pub fn activity(&self) -> &ActivityPage {
        &self.activity
    }

    pub fn routines(&self) -> &RoutinesPage {
        &self.routines
    }

    /// Redraws the Scheduled page and the active count in the sidebar.
    pub fn render_routines(&self, routines: &Routines, running: Option<u64>, now: u64) {
        self.routines.render(routines, running, now);
        self.sidebar.set_active_routines(routines.active_count());
    }

    pub fn autopilot(&self) -> &AutopilotPage {
        &self.autopilot
    }

    /// Redraws the Autopilot page and the count of jobs waiting in the sidebar.
    pub fn render_autopilot(&self, jobs: &Jobs, enabled: bool, now: u64) {
        self.autopilot.render(jobs, enabled, now);
        self.sidebar.set_waiting_jobs(jobs.waiting_count());
    }

    pub fn set_unread_activity(&self, count: usize) {
        self.sidebar.set_unread_activity(count);
    }

    /// Redraws the Activity page and the unread count in the sidebar.
    pub fn render_activity(&self, log: &ActivityLog, now: u64) {
        self.activity.render(log, now);
        self.sidebar.set_unread_activity(log.unread_count());
    }

    /// Puts a draft in the Slack reply box and focuses it.
    pub fn put_slack_draft(&self, text: &str) {
        self.slack.set_input(text, &self.panel);
    }

    pub fn is_visible(&self) -> bool {
        self.panel.isVisible()
    }

    pub fn is_key(&self) -> bool {
        self.panel.isKeyWindow()
    }

    pub fn page(&self) -> Page {
        self.page.get()
    }

    /// Switches the right side to `page` and puts the cursor in its input.
    pub fn show_page(&self, page: Page) {
        self.page.set(page);
        self.chat.view.setHidden(page != Page::Chat);
        self.tasks.view.setHidden(page != Page::Tasks);
        self.plan.view.setHidden(page != Page::Plan);
        self.slack.view.setHidden(page != Page::Slack);
        self.activity.view.setHidden(page != Page::Activity);
        self.routines.view.setHidden(page != Page::Routines);
        self.autopilot.view.setHidden(page != Page::Autopilot);
        self.refresh_header();
        self.focus();
    }

    /// The top line of the page shown: its name on the left, its status on the right.
    fn refresh_header(&self) {
        let page = self.page();
        let (title, selection, can_run) = match page {
            // A plan is a quick action's result: same title and [ RUN ] as its chat.
            Page::Chat | Page::Plan => (
                self.chat_title.borrow().clone(),
                self.chat_selection.get(),
                self.can_run.get(),
            ),
            Page::Tasks => (self.tasks_title.borrow().clone(), Selection::Tasks, false),
            Page::Slack => ("Slack".to_string(), Selection::Slack, false),
            Page::Activity => ("Activity".to_string(), Selection::Activity, false),
            Page::Routines => ("Scheduled".to_string(), Selection::Routines, false),
            Page::Autopilot => ("Autopilot".to_string(), Selection::Autopilot, false),
        };
        self.sidebar.select(selection);
        self.run_button.setHidden(!can_run);
        let (name, status) = match title.split_once(" · ") {
            Some((name, status)) => (name.to_string(), Some(status.to_string())),
            None => (title, None),
        };
        let status = if page == Page::Tasks {
            let open = format!("{} open", self.open_tasks.get());
            Some(status.map_or(open.clone(), |status| format!("{open} · {status}")))
        } else {
            status
        };
        self.header.setAttributedStringValue(&rich_text::attributed(
            &name,
            &Attributes::plain(theme::highlight(), theme::title_font(theme::TITLE_SIZE)),
        ));
        self.subtitle
            .setAttributedStringValue(&rich_text::attributed(
                status.as_deref().unwrap_or(""),
                &Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE)),
            ));
        self.layout_subtitle();
    }

    /// Pins the status to the top-right corner of the page, just left of the
    /// close button (or of RUN when shown), centered on the title's line. It
    /// never starts before the end of the title.
    fn layout_subtitle(&self) {
        let row = self.header.frame();
        let run = self.run_button.frame();
        let right = if self.run_button.isHidden() {
            run.origin.x + run.size.width
        } else {
            run.origin.x
        } - SUBTITLE_GAP / 2.0;
        let title_end = row.origin.x + self.header.fittingSize().width.min(row.size.width);
        let size = self.subtitle.fittingSize();
        let width = size.width.min((right - title_end - SUBTITLE_GAP).max(0.0));
        self.subtitle.setFrame(rect(
            right - width,
            row.origin.y + ((row.size.height - size.height) / 2.0).max(0.0),
            width,
            size.height,
        ));
    }

    /// Names the conversation shown on the chat or plan page, the sidebar
    /// entry it belongs to, and whether it is a quick action that can be run again.
    pub fn set_chat_title(&self, title: &str, selection: Selection, can_run: bool) {
        self.chat_title.replace(title.to_string());
        self.chat_selection.set(selection);
        self.can_run.set(can_run);
        if self.page() != Page::Tasks {
            self.show_page(self.page());
        }
    }

    /// Header of the Tasks page, e.g. "Tasks · checked 2 min ago".
    pub fn set_tasks_title(&self, title: &str) {
        if *self.tasks_title.borrow() == title {
            return;
        }
        self.tasks_title.replace(title.to_string());
        if self.page() == Page::Tasks {
            self.refresh_header();
        }
    }

    fn focus(&self) {
        match self.page() {
            Page::Chat => self.chat.focus(&self.panel),
            Page::Tasks => self.tasks.focus(&self.panel),
            Page::Plan => self.plan.focus(&self.panel),
            Page::Slack => self.slack.focus(&self.panel),
            // Nothing to type into: the bubble itself keeps the focus.
            Page::Activity | Page::Routines | Page::Autopilot => {}
        }
    }

    /// Redraws the task page and the open count in the sidebar.
    pub fn render_tasks(&self, tasks: &TaskList, now: u64) {
        self.tasks.render(tasks, now);
        self.sidebar.set_open_tasks(tasks.open_count());
        self.open_tasks.set(tasks.open_count());
        if self.page() == Page::Tasks {
            self.refresh_header();
        }
    }

    /// Sets the bubble's size (`width`, `height` in points); it is fitted to
    /// the screen when it opens.
    pub fn set_size(&self, (width, height): (f64, f64)) {
        self.unfitted.set(None);
        self.panel.setContentSize(NSSize::new(width, height));
    }

    /// Opens the bubble just above `anchor` (the cat), kept on screen, with
    /// its tail pointing at the cat.
    pub fn show_above(&self, anchor: NSRect, mtm: MainThreadMarker) {
        if let Some(wanted) = self.unfitted.take() {
            self.panel.setContentSize(wanted);
        }
        let mut size = self.panel.frame().size;
        let cat_center = anchor.origin.x + anchor.size.width / 2.0;
        let mut x = cat_center - size.width / 2.0;
        let mut y = anchor.origin.y + anchor.size.height;
        let anchor_center = NSPoint::new(cat_center, anchor.origin.y + anchor.size.height / 2.0);
        if let Some(screen) = screens::showing(mtm, anchor_center) {
            let visible = screen.visibleFrame();
            // A big bubble on a small screen shrinks to fit above the cat.
            let room = NSSize::new(
                visible.size.width,
                visible.origin.y + visible.size.height - y,
            );
            if size.width > room.width || size.height > room.height {
                self.unfitted.set(Some(size));
                size = NSSize::new(
                    size.width.min(room.width).max(MIN_SIZE.width),
                    size.height.min(room.height).max(MIN_SIZE.height),
                );
                self.panel.setContentSize(size);
            }
            x = cat_center - size.width / 2.0;
            x = x.clamp(
                visible.origin.x,
                visible.origin.x + visible.size.width - size.width,
            );
            y = y.min(visible.origin.y + visible.size.height - size.height);
        }
        self.panel.setFrameOrigin(NSPoint::new(x, y));
        self.bubble.set_tail_x(cat_center - x);
        self.panel.makeKeyAndOrderFront(None);
        self.focus();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    /// Saves the bubble as it looks now to a PNG at `path` (debug snapshots).
    pub fn snapshot(&self, path: &std::path::Path) -> Result<(), String> {
        let view: &NSView = &self.bubble;
        view.displayIfNeeded();
        let bounds = view.bounds();
        let rep = view
            .bitmapImageRepForCachingDisplayInRect(bounds)
            .ok_or("no bitmap for the bubble")?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        // SAFETY: an empty properties dictionary is valid for PNG output.
        let data = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }
        .ok_or("could not encode the PNG")?;
        std::fs::write(path, data.to_vec()).map_err(|e| e.to_string())
    }

    pub fn set_busy(&self, busy: bool) {
        self.sidebar.set_busy(busy);
    }

    /// The cat beside the title takes the coat chosen for the cat.
    pub fn set_coat(&self, coat: meow::sprites::Coat) {
        self.sidebar.set_coat(coat);
    }
}

fn make_panel(mtm: MainThreadMarker) -> Retained<BubblePanel> {
    let panel = BubblePanel::new(mtm, NSSize::new(WIDTH, HEIGHT));
    // SAFETY: the panel is owned by `ChatWindow` for the app's lifetime.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setTitle(&NSString::from_str("Meow"));
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    // Dark controls (fields, checkboxes, scrollers) on the dark bubble.
    // SAFETY: AppKit's appearance-name constant is an immutable static.
    panel.setAppearance(
        NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref(),
    );
    // The bubble draws its own pixel shadow.
    panel.setHasShadow(false);
    panel.setMovableByWindowBackground(true);
    panel.setFloatingPanel(true);
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(false);
    panel.setLevel(NSStatusWindowLevel);
    panel.setMinSize(MIN_SIZE);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel
}

/// The bubble has no title bar, so it gets its own small close button.
fn close_button(
    mtm: MainThreadMarker,
    controller: &Controller,
    inner: NSRect,
) -> Retained<NSButton> {
    let target: &AnyObject = controller;
    // SAFETY: the controller is an app-lifetime target that implements `toggleChat:`.
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(""),
            Some(target),
            Some(sel!(toggleChat:)),
            mtm,
        )
    };
    button.setBordered(false);
    button.setAttributedTitle(&rich_text::attributed(
        "✕",
        &Attributes::plain(theme::ink(), theme::font(24.0)),
    ));
    button.setToolTip(Some(&NSString::from_str("Close (Esc)")));
    button.setFrame(rect(
        inner.origin.x + inner.size.width - CLOSE_SIZE,
        inner.origin.y,
        CLOSE_SIZE,
        CLOSE_SIZE,
    ));
    // The bubble view is flipped: a flexible bottom margin keeps it at the top.
    button.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
    );
    button
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
