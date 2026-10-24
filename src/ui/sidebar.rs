//! Left column of the bubble: title, pages (Chat, Tasks, Slack, Activity) and
//! the quick actions.

use std::cell::Cell;

use meow::actions::QuickAction;
use meow::sprites::{self, Bubble, Coat, Facing, Look, Pose};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezierPath, NSButton, NSColor, NSFont, NSImage, NSImageView,
    NSTextAlignment, NSTextField,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use super::controller::Controller;
use super::flipped::FlippedView;
use super::rich_text::{self, Attributes};
use super::theme;
use super::widgets;

pub const WIDTH: f64 = 156.0;
const PADDING: f64 = 6.0;
const TITLE_HEIGHT: f64 = 38.0;
const NAV_HEIGHT: f64 = 28.0;
const ACTION_HEIGHT: f64 = 26.0;
const SECTION_GAP: f64 = 16.0;
const NAV_SIZE: f64 = 13.0;
const ACTION_SIZE: f64 = 17.0;
/// The square ahead of a quick action, smaller than its title.
const BULLET_SIZE: f64 = 11.0;
/// The cat beside the MEOW title, as tall as the title row allows.
const ICON_HEIGHT: f64 = 26.0;
const ICON_GAP: f64 = 8.0;
const BADGE_WIDTH: f64 = 34.0;
const NEW_HEIGHT: f64 = 32.0;
/// Bullet colors of the quick actions, in turn.
const BULLETS: [fn() -> Retained<NSColor>; 4] =
    [theme::highlight, theme::link, theme::live, theme::accent];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Chat,
    Tasks,
    /// The day plan of the quick action shown.
    Plan,
    /// Slack conversations.
    Slack,
    /// What happened in the background.
    Activity,
    /// Scheduled tasks and watches.
    Routines,
    /// Work started from Slack messages, and what waits for approval.
    Autopilot,
}

/// What the sidebar highlights: a page, or the quick action shown on the chat page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Chat,
    Tasks,
    Slack,
    Activity,
    Routines,
    Autopilot,
    Action(usize),
}

pub struct Sidebar {
    pub view: Retained<FlippedView>,
    icon: Retained<NSImageView>,
    title: Retained<NSTextField>,
    /// The open task count, a gold chip at the end of the Tasks row.
    tasks_badge: Retained<NSTextField>,
    /// The unread activity count, an orange chip at the end of the Activity row.
    activity_badge: Retained<NSTextField>,
    /// The active scheduled task count, a blue chip at the end of the Scheduled row.
    routines_badge: Retained<NSTextField>,
    /// The jobs waiting for approval, an orange chip at the end of the Autopilot row.
    autopilot_badge: Retained<NSTextField>,
    chat: Retained<NSButton>,
    tasks: Retained<NSButton>,
    slack: Retained<NSButton>,
    activity: Retained<NSButton>,
    routines: Retained<NSButton>,
    autopilot: Retained<NSButton>,
    actions: Vec<(Retained<NSButton>, String)>,
    selected: Cell<Selection>,
    open_tasks: Cell<usize>,
    unread_activity: Cell<usize>,
    active_routines: Cell<usize>,
    waiting_jobs: Cell<usize>,
    busy: Cell<bool>,
}

impl Sidebar {
    /// Builds the column at the left of `area` (the bubble's content area).
    pub fn new(
        mtm: MainThreadMarker,
        controller: &Controller,
        actions: &[QuickAction],
        area: NSRect,
    ) -> Self {
        let view = FlippedView::new(
            mtm,
            rect(area.origin.x, area.origin.y, WIDTH, area.size.height),
        );
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);

        let icon = NSImageView::initWithFrame(
            NSImageView::alloc(mtm),
            rect(
                PADDING,
                (TITLE_HEIGHT - ICON_HEIGHT) / 2.0,
                ICON_HEIGHT,
                ICON_HEIGHT,
            ),
        );
        view.addSubview(&icon);

        let title = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        let title_x = PADDING + ICON_HEIGHT + ICON_GAP;
        title.setFrame(rect(
            title_x,
            (TITLE_HEIGHT - 20.0) / 2.0,
            WIDTH - title_x - PADDING,
            20.0,
        ));
        view.addSubview(&title);

        let target: &AnyObject = controller;
        let mut y = TITLE_HEIGHT + PADDING;
        let chat = borderless_button(mtm, target, sel!(showChat:), y, NAV_HEIGHT);
        y += NAV_HEIGHT;
        let tasks = borderless_button(mtm, target, sel!(showTasks:), y, NAV_HEIGHT);
        y += NAV_HEIGHT;
        let slack = borderless_button(mtm, target, sel!(showSlack:), y, NAV_HEIGHT);
        y += NAV_HEIGHT;
        let activity = borderless_button(mtm, target, sel!(showActivity:), y, NAV_HEIGHT);
        y += NAV_HEIGHT;
        let routines = borderless_button(mtm, target, sel!(showRoutines:), y, NAV_HEIGHT);
        y += NAV_HEIGHT;
        let autopilot = borderless_button(mtm, target, sel!(showAutopilot:), y, NAV_HEIGHT);
        y += NAV_HEIGHT + SECTION_GAP;
        view.addSubview(&chat);
        view.addSubview(&tasks);
        view.addSubview(&slack);
        view.addSubview(&activity);
        view.addSubview(&routines);
        view.addSubview(&autopilot);

        let tasks_badge = badge(mtm, &tasks, &theme::highlight());
        view.addSubview(&tasks_badge);
        let activity_badge = badge(mtm, &activity, &theme::accent());
        view.addSubview(&activity_badge);
        let routines_badge = badge(mtm, &routines, &theme::link());
        view.addSubview(&routines_badge);
        let autopilot_badge = badge(mtm, &autopilot, &theme::accent());
        view.addSubview(&autopilot_badge);

        if !actions.is_empty() {
            let label = NSTextField::labelWithString(&NSString::from_str("QUICK ACTIONS"), mtm);
            label.setFont(Some(&theme::label_font(theme::LABEL_SIZE)));
            label.setTextColor(Some(&theme::muted()));
            label.setFrame(rect(PADDING, y, WIDTH - 2.0 * PADDING, 20.0));
            view.addSubview(&label);
            y += 22.0;
        }
        let mut action_buttons = Vec::new();
        for (index, action) in actions.iter().enumerate() {
            let button = borderless_button(mtm, target, sel!(quickAction:), y, ACTION_HEIGHT);
            button.setTag(index as isize);
            button.setToolTip(Some(&NSString::from_str(&format!(
                "Show the last result of {}",
                action.title
            ))));
            view.addSubview(&button);
            action_buttons.push((button, action.title.clone()));
            y += ACTION_HEIGHT;
        }

        // Starts a fresh conversation, like ⌘N; kept at the bottom.
        let new = widgets::text_button(
            mtm,
            "+ New   ⌘N",
            widgets::ButtonStyle::Quiet,
            target,
            sel!(newConversation:),
        );
        new.setFrame(rect(
            PADDING,
            area.size.height - PADDING - NEW_HEIGHT,
            WIDTH - 3.0 * PADDING,
            NEW_HEIGHT,
        ));
        // The column is flipped: a flexible top margin keeps it at the bottom.
        new.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
        view.addSubview(&new);

        let sidebar = Self {
            view,
            icon,
            title,
            tasks_badge,
            activity_badge,
            routines_badge,
            autopilot_badge,
            chat,
            tasks,
            slack,
            activity,
            routines,
            autopilot,
            actions: action_buttons,
            selected: Cell::new(Selection::Chat),
            open_tasks: Cell::new(0),
            unread_activity: Cell::new(0),
            active_routines: Cell::new(0),
            waiting_jobs: Cell::new(0),
            busy: Cell::new(false),
        };
        sidebar.refresh();
        sidebar
    }

    pub fn select(&self, selection: Selection) {
        self.selected.set(selection);
        self.refresh();
    }

    pub fn set_open_tasks(&self, count: usize) {
        self.open_tasks.set(count);
        self.refresh();
    }

    /// Draws the cat beside the title in `coat`.
    pub fn set_active_routines(&self, count: usize) {
        self.active_routines.set(count);
        self.refresh();
    }

    pub fn set_waiting_jobs(&self, count: usize) {
        self.waiting_jobs.set(count);
        self.refresh();
    }

    pub fn set_unread_activity(&self, count: usize) {
        self.unread_activity.set(count);
        self.refresh();
    }

    pub fn set_coat(&self, coat: Coat) {
        let look = Look {
            pose: Pose::Stand,
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        if let Some(image) = sprite_image(&sprites::render(&look, coat), ICON_HEIGHT) {
            self.icon.setImage(Some(&image));
        }
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.refresh();
    }

    fn refresh(&self) {
        let title = if self.busy.get() {
            "MEOW ···"
        } else {
            "MEOW"
        };
        self.title.setAttributedStringValue(&rich_text::attributed(
            title,
            &Attributes::plain(theme::accent(), theme::title_font(theme::TITLE_SIZE)),
        ));
        set_badge(&self.tasks_badge, self.open_tasks.get());
        set_badge(&self.activity_badge, self.unread_activity.get());
        set_badge(&self.routines_badge, self.active_routines.get());
        set_badge(&self.autopilot_badge, self.waiting_jobs.get());
        let selected = self.selected.get();
        let pages = [
            (&self.chat, "Chat", Selection::Chat),
            (&self.tasks, "Tasks", Selection::Tasks),
            (&self.slack, "Slack", Selection::Slack),
            (&self.activity, "Activity", Selection::Activity),
            (&self.routines, "Scheduled", Selection::Routines),
            (&self.autopilot, "Autopilot", Selection::Autopilot),
        ];
        for (button, text, selection) in pages {
            set_entry(
                button,
                text,
                theme::label_font(NAV_SIZE),
                selected == selection,
            );
        }
        for (index, (button, title)) in self.actions.iter().enumerate() {
            let on = selected == Selection::Action(index);
            set_entry(button, title, theme::font(ACTION_SIZE), on);
            // A colored square ahead of each action, as on the design.
            let bullet = rich_text::attributed(
                " ■ ",
                &Attributes::plain(BULLETS[index % BULLETS.len()](), theme::font(BULLET_SIZE)),
            );
            let title = objc2_foundation::NSMutableAttributedString::new();
            title.appendAttributedString(&bullet);
            title.appendAttributedString(&button.attributedTitle());
            button.setAttributedTitle(&title);
        }
    }
}

/// A count chip at the right end of nav row `row`.
fn badge(mtm: MainThreadMarker, row: &NSButton, fill: &NSColor) -> Retained<NSTextField> {
    let badge = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    badge.setAlignment(NSTextAlignment::Center);
    badge.setFrame(rect(
        WIDTH - 2.0 * PADDING - BADGE_WIDTH,
        row.frame().origin.y + (NAV_HEIGHT - 18.0) / 2.0,
        BADGE_WIDTH,
        18.0,
    ));
    widgets::paint(&badge, Some(fill), None);
    badge
}

/// Shows `count` in `badge`, hidden at zero.
fn set_badge(badge: &NSTextField, count: usize) {
    badge.setHidden(count == 0);
    badge.setAttributedStringValue(&rich_text::attributed(
        &count.to_string(),
        &Attributes::plain(theme::on_accent(), theme::label_font(theme::LABEL_SIZE)),
    ));
}

/// The selected entry is a full-width dark-on-orange row, the others plain ink.
fn set_entry(button: &NSButton, text: &str, font: Retained<NSFont>, selected: bool) {
    let color = if selected {
        theme::on_accent()
    } else {
        theme::ink()
    };
    button.setAttributedTitle(&rich_text::attributed(
        &format!(" {text}"),
        &Attributes::plain(color, font),
    ));
    widgets::paint(button, selected.then(theme::accent).as_deref(), None);
}

/// The visible pixels of `frame`, drawn square at the largest whole scale
/// that fits `height` points; `None` if nothing is visible.
fn sprite_image(frame: &sprites::Frame, height: f64) -> Option<Retained<NSImage>> {
    let visible = |x: usize, y: usize| frame.get(x, y).is_some();
    let rows: Vec<usize> = (0..frame.height)
        .filter(|&y| (0..frame.width).any(|x| visible(x, y)))
        .collect();
    let cols: Vec<usize> = (0..frame.width)
        .filter(|&x| (0..frame.height).any(|y| visible(x, y)))
        .collect();
    let (top, bottom) = (*rows.first()?, *rows.last()?);
    let (left, right) = (*cols.first()?, *cols.last()?);
    let (w, h) = (right - left + 1, bottom - top + 1);
    let scale = height / h as f64;
    let image = NSImage::initWithSize(
        NSImage::alloc(),
        NSSize::new(w as f64 * scale, h as f64 * scale),
    );
    #[allow(deprecated)]
    image.lockFocus();
    for y in top..=bottom {
        for x in left..=right {
            let Some([r, g, b]) = frame.get(x, y) else {
                continue;
            };
            let channel = |v: u8| f64::from(v) / 255.0;
            NSColor::colorWithSRGBRed_green_blue_alpha(channel(r), channel(g), channel(b), 1.0)
                .setFill();
            // Image space grows upward; sprite rows go down.
            let origin = NSPoint::new((x - left) as f64 * scale, (bottom - y) as f64 * scale);
            NSBezierPath::fillRect(NSRect::new(origin, NSSize::new(scale, scale)));
        }
    }
    #[allow(deprecated)]
    image.unlockFocus();
    Some(image)
}

fn borderless_button(
    mtm: MainThreadMarker,
    target: &AnyObject,
    action: Sel,
    y: f64,
    height: f64,
) -> Retained<NSButton> {
    // SAFETY: the controller is an app-lifetime target that implements every sidebar action.
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(""),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setBordered(false);
    button.setAlignment(NSTextAlignment::Left);
    button.setFrame(rect(PADDING, y, WIDTH - 2.0 * PADDING, height));
    button
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
