//! Left column of the bubble: title, pages (Chat, Tasks, Activity and the
//! features' pages, in their order) and the quick actions.

use std::cell::Cell;

use meow::actions::QuickAction;
use meow::sprites::{self, Bubble, Coat, Facing, Look, Pose};
use meow::vector_cat;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezierPath, NSButton, NSColor, NSFont, NSImage, NSImageView,
    NSTextAlignment, NSTextField,
};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use super::action::Action;
use super::controller::Controller;
use super::events::{self, UiEvent};
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
    /// What happened in the background.
    Activity,
    /// A feature's page, by the id in its `PageSpec`.
    Feature(&'static str),
}

/// What the sidebar highlights: a page, or the quick action shown on the chat page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Chat,
    Tasks,
    Activity,
    Feature(&'static str),
    Action(usize),
}

/// A page a feature adds to the sidebar (see `Feature::page`).
#[derive(Clone, Copy)]
pub struct PageSpec {
    /// Names the page in `Page::Feature`.
    pub id: &'static str,
    /// Its sidebar entry and header.
    pub title: &'static str,
    /// The fill of the count chip at the end of its row.
    pub badge: fn() -> Retained<NSColor>,
    /// Where its row goes among the others, smallest first: the app's own
    /// are Chat at 10, Tasks at 20 and Activity at 40.
    pub order: u8,
}

/// The places of the app's own rows (see `PageSpec::order`).
const CHAT_ORDER: u8 = 10;
const TASKS_ORDER: u8 = 20;
const ACTIVITY_ORDER: u8 = 40;

/// A feature page's row: its button, count chip and count.
struct FeatureRow {
    spec: PageSpec,
    button: Retained<NSButton>,
    badge: Retained<NSTextField>,
    count: Cell<usize>,
    /// The button's target, which the button only holds weakly.
    _action: Retained<Action>,
}

pub struct Sidebar {
    pub view: Retained<FlippedView>,
    icon: Retained<NSImageView>,
    title: Retained<NSTextField>,
    /// The open task count, a gold chip at the end of the Tasks row.
    tasks_badge: Retained<NSTextField>,
    /// The unread activity count, an orange chip at the end of the Activity row.
    activity_badge: Retained<NSTextField>,
    chat: Retained<NSButton>,
    tasks: Retained<NSButton>,
    activity: Retained<NSButton>,
    pages: Vec<FeatureRow>,
    actions: Vec<(Retained<NSButton>, String)>,
    selected: Cell<Selection>,
    open_tasks: Cell<usize>,
    unread_activity: Cell<usize>,
    busy: Cell<bool>,
}

impl Sidebar {
    /// Builds the column at the left of `area` (the bubble's content area),
    /// with a row for each of the features' `pages` below the app's own.
    pub fn new(
        mtm: MainThreadMarker,
        controller: &Controller,
        actions: &[QuickAction],
        pages: &[PageSpec],
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
        let chat = borderless_button(mtm, target, sel!(showChat:), 0.0, NAV_HEIGHT);
        let tasks = borderless_button(mtm, target, sel!(showTasks:), 0.0, NAV_HEIGHT);
        let activity = borderless_button(mtm, target, sel!(showActivity:), 0.0, NAV_HEIGHT);
        let feature_buttons: Vec<_> = pages
            .iter()
            .map(|&spec| (spec, feature_button(mtm, spec)))
            .collect();
        // The rows top to bottom, the app's own among the features' by order.
        let mut rows: Vec<(u8, &NSButton)> = vec![
            (CHAT_ORDER, &chat),
            (TASKS_ORDER, &tasks),
            (ACTIVITY_ORDER, &activity),
        ];
        rows.extend(
            feature_buttons
                .iter()
                .map(|(spec, (button, _))| (spec.order, &**button)),
        );
        rows.sort_by_key(|&(order, _)| order);
        let mut y = TITLE_HEIGHT + PADDING;
        for (_, button) in rows {
            button.setFrameOrigin(NSPoint::new(PADDING, y));
            view.addSubview(button);
            y += NAV_HEIGHT;
        }
        y += SECTION_GAP;

        let tasks_badge = badge(mtm, &tasks, &theme::highlight());
        view.addSubview(&tasks_badge);
        let activity_badge = badge(mtm, &activity, &theme::accent());
        view.addSubview(&activity_badge);
        let pages: Vec<_> = feature_buttons
            .into_iter()
            .map(|(spec, (button, action))| {
                let badge = badge(mtm, &button, &(spec.badge)());
                view.addSubview(&badge);
                FeatureRow {
                    spec,
                    button,
                    badge,
                    count: Cell::new(0),
                    _action: action,
                }
            })
            .collect();

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
            chat,
            tasks,
            activity,
            pages,
            actions: action_buttons,
            selected: Cell::new(Selection::Chat),
            open_tasks: Cell::new(0),
            unread_activity: Cell::new(0),
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

    /// The count at the end of feature page `page`'s row; hidden at zero.
    pub fn set_badge(&self, page: &str, count: usize) {
        if let Some(row) = self.pages.iter().find(|row| row.spec.id == page) {
            row.count.set(count);
            self.refresh();
        }
    }

    pub fn set_unread_activity(&self, count: usize) {
        self.unread_activity.set(count);
        self.refresh();
    }

    /// Draws the cat beside the title in `coat`.
    pub fn set_coat(&self, coat: Coat) {
        let look = Look {
            pose: Pose::Stand,
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        let image = if meow::style::is_classic() {
            vector_image(&look, coat, ICON_HEIGHT)
        } else {
            sprite_image(&sprites::render(&look, coat), ICON_HEIGHT)
        };
        if let Some(image) = image {
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
        let selected = self.selected.get();
        let pages = [
            (&self.chat, "Chat", Selection::Chat),
            (&self.tasks, "Tasks", Selection::Tasks),
            (&self.activity, "Activity", Selection::Activity),
        ];
        let features = self.pages.iter().map(|row| {
            set_badge(&row.badge, row.count.get());
            (&row.button, row.spec.title, Selection::Feature(row.spec.id))
        });
        for (button, text, selection) in pages.into_iter().chain(features) {
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

/// The button of a feature page's row, and its target: clicking it shows
/// the page.
fn feature_button(mtm: MainThreadMarker, spec: PageSpec) -> (Retained<NSButton>, Retained<Action>) {
    let page = Page::Feature(spec.id);
    let action = Action::new(mtm, move || events::push(UiEvent::ShowPage(page)));
    let button = borderless_button(mtm, &action, Action::selector(), 0.0, NAV_HEIGHT);
    (button, action)
}

/// The flat cat of the Classic style, without the bubble space above it,
/// `height` points tall and drawn sharp on Retina screens.
fn vector_image(look: &Look, coat: Coat, height: f64) -> Option<Retained<NSImage>> {
    let rows = (sprites::CANVAS_H - sprites::BUBBLE_H) as f64;
    let width = height * sprites::CANVAS_W as f64 / rows;
    let scale = 2.0 * height / rows;
    let pixels = |points: f64| (2.0 * points).round() as usize;
    let bitmap = vector_cat::Bitmap::new(pixels(width), pixels(height))?;
    bitmap.draw(scale, |ctx| {
        CGContext::translate_ctm(Some(ctx), 0.0, -(sprites::BUBBLE_H as f64));
        vector_cat::draw_look(ctx, look, coat);
    });
    let image = bitmap.image()?;
    Some(NSImage::initWithCGImage_size(
        NSImage::alloc(),
        &image,
        NSSize::new(width, height),
    ))
}

fn borderless_button(
    mtm: MainThreadMarker,
    target: &AnyObject,
    action: Sel,
    y: f64,
    height: f64,
) -> Retained<NSButton> {
    // SAFETY: the target implements `action` and lives as long as the sidebar: the
    // app-lifetime controller, or an `Action` kept in the row.
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
