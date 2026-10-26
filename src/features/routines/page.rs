//! The Scheduled page: tasks asked for in the chat ("at 17:00, do X", "when
//! Y happens, tell me"), active ones first, each with when it runs, what it
//! does, its last result and a × to delete it.

use std::cell::RefCell;
use std::rc::Rc;

use meow::routines::{Routine, Routines, Trigger};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSLineBreakMode, NSScrollView, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSAttributedString, NSPoint, NSRect, NSSize, NSString};

use crate::ui::action::Action;
use crate::ui::bubble;
use crate::ui::flipped::FlippedView;
use crate::ui::rich_text::{self, Attributes};
use crate::ui::theme;
use crate::ui::widgets;

const MARGIN: f64 = 16.0;
const ROW_HEIGHT: f64 = 22.0;
const LINE_HEIGHT: f64 = 18.0;
const CARD_PADDING: f64 = 8.0;
const CARD_GAP: f64 = 6.0;
const WHEN_WIDTH: f64 = 190.0;
const DELETE_WIDTH: f64 = 22.0;
const INDENT: f64 = 6.0;
const RUNNING_BORDER: f64 = 2.0;

pub struct RoutinesPage {
    pub view: Retained<NSView>,
    scroll: Retained<NSScrollView>,
    list: Retained<FlippedView>,
    /// Called with a routine's id when its × is clicked.
    on_delete: Rc<dyn Fn(u64)>,
    /// The × buttons' targets, which the buttons only hold weakly.
    actions: RefCell<Vec<Retained<Action>>>,
    mtm: MainThreadMarker,
}

impl RoutinesPage {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, on_delete: impl Fn(u64) + 'static) -> Self {
        let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let (width, height) = (frame.size.width - bubble::PADDING, frame.size.height);
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            rect(MARGIN, MARGIN, width - 2.0 * MARGIN, height - 1.5 * MARGIN),
        );
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let list = FlippedView::new(mtm, rect(0.0, 0.0, scroll.contentSize().width, 0.0));
        list.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        scroll.setDocumentView(Some(&list));
        view.addSubview(&scroll);
        Self {
            view,
            scroll,
            list,
            on_delete: Rc::new(on_delete),
            actions: RefCell::new(Vec::new()),
            mtm,
        }
    }

    /// Draws the routines; the one `running` gets an orange outline.
    pub fn render(&self, routines: &Routines, running: Option<u64>, now: u64) {
        let width = self.scroll.contentSize().width;
        for row in self.list.subviews().iter() {
            row.removeFromSuperview();
        }
        let shown = routines.display();
        self.actions.borrow_mut().clear();
        let mut y = 0.0;
        if shown.is_empty() {
            let hint = "Nothing scheduled. Ask in the chat, e.g. \"at 17:00, summarize my \
                        unread emails\" or \"when Staples replies to Cyril's email, let me know\".";
            let style = Attributes::plain(theme::muted(), theme::font(theme::BODY_SIZE));
            let label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), self.mtm);
            label.setAttributedStringValue(&rich_text::attributed(hint, &style));
            label.setFrame(rect(INDENT, 4.0, width - 2.0 * INDENT, 3.0 * LINE_HEIGHT));
            label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
            self.list.addSubview(&label);
            y = 3.0 * LINE_HEIGHT;
        }
        for routine in shown {
            let is_running = running == Some(routine.id);
            y = self.add_routine(routine, is_running, width, y, now) + CARD_GAP;
        }
        let height = y.max(self.scroll.contentSize().height);
        self.list.setFrameSize(NSSize::new(width, height));
    }

    /// A card: name and when on top, then what it watches and does, then its
    /// last result. Returns the y below.
    fn add_routine(&self, routine: &Routine, running: bool, width: f64, y: f64, now: u64) -> f64 {
        let lines = describe(routine, running);
        let height = ROW_HEIGHT + lines.len() as f64 * LINE_HEIGHT + 2.0 * CARD_PADDING;
        let card = FlippedView::new(self.mtm, rect(0.0, y, width, height));
        card.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let fill = if routine.is_active() {
            theme::card()
        } else {
            theme::card_done()
        };
        let border = running.then(theme::accent);
        widgets::paint(
            &card,
            Some(&fill),
            border.as_deref().map(|color| (color, RUNNING_BORDER)),
        );
        self.list.addSubview(&card);
        let top = y + CARD_PADDING;

        let delete_x = width - INDENT - DELETE_WIDTH;
        let when_x = delete_x - WHEN_WIDTH;
        let color = if routine.is_active() {
            theme::ink()
        } else {
            theme::faint()
        };
        let name = Attributes {
            heavy: routine.is_active(),
            ..Attributes::plain(color, theme::font(theme::BODY_SIZE))
        };
        self.add_label(
            &rich_text::attributed(&routine.name, &name),
            rect(2.0 * INDENT, top, when_x - 3.0 * INDENT, ROW_HEIGHT),
        );
        let when = if routine.is_active() {
            routine.schedule_label(now)
        } else {
            "done".to_string()
        };
        let small = Attributes::plain(theme::faint(), theme::font(theme::SMALL_SIZE));
        let label = self.add_label(
            &rich_text::attributed(&when, &small),
            rect(when_x, top + 3.0, WHEN_WIDTH, ROW_HEIGHT - 3.0),
        );
        label.setAlignment(NSTextAlignment::Right);
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);

        let (on_delete, id) = (Rc::clone(&self.on_delete), routine.id);
        let action = Action::new(self.mtm, move || on_delete(id));
        let delete = widgets::text_button(
            self.mtm,
            "×",
            widgets::ButtonStyle::Link,
            &action,
            Action::selector(),
        );
        self.actions.borrow_mut().push(action);
        delete.setToolTip(Some(&NSString::from_str("Delete this scheduled task")));
        delete.setFrame(rect(delete_x, top, DELETE_WIDTH, ROW_HEIGHT));
        delete.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        self.list.addSubview(&delete);

        let muted = Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE));
        for (n, line) in lines.iter().enumerate() {
            let label = self.add_label(
                &rich_text::attributed(line, &muted),
                rect(
                    2.0 * INDENT,
                    top + ROW_HEIGHT + n as f64 * LINE_HEIGHT,
                    width - 4.0 * INDENT,
                    LINE_HEIGHT,
                ),
            );
            label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        }
        y + height
    }

    fn add_label(&self, text: &NSAttributedString, frame: NSRect) -> Retained<NSTextField> {
        let label = NSTextField::labelWithAttributedString(text, self.mtm);
        label.setFrame(frame);
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        self.list.addSubview(&label);
        label
    }
}

/// The lines under a routine's name: what it watches, what it does, how it went.
fn describe(routine: &Routine, running: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Trigger::When {
        condition, once, ..
    } = &routine.trigger
    {
        let repeat = if *once { "" } else { " (every time)" };
        lines.push(format!("When: {condition}{repeat}"));
    }
    let then = if routine.notify_only {
        format!("Then: let you know · {}", routine.then)
    } else {
        format!("Then: {}", routine.then)
    };
    lines.push(then);
    if running {
        lines.push("Working on it…".to_string());
    } else if let Some(result) = &routine.last_result {
        lines.push(format!("Last: {result}"));
    }
    lines
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
