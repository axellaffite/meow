//! The "write like me" controls above the Slack conversation list: a toggle,
//! a button to (re)learn the user's style from their messages, and a status.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSButton, NSTextField, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use super::action::{Action, ActionSink};
use super::rich_text::{self, Attributes};
use super::slack_page::SlackAction;
use super::theme;
use super::widgets;

pub const HEIGHT: f64 = 44.0;
const BUTTON_HEIGHT: f64 = 28.0;
const TOGGLE_WIDTH: f64 = 178.0;
const LEARN_WIDTH: f64 = 120.0;
const GAP: f64 = 10.0;
const PADDING: f64 = 8.0;

/// What the bar shows.
pub struct VoiceBarState<'a> {
    /// Drafts follow the style guide.
    pub enabled: bool,
    /// A style guide has been learned.
    pub learned: bool,
    /// Learning is under way.
    pub busy: bool,
    pub status: &'a str,
}

pub struct VoiceBar {
    pub view: Retained<NSView>,
    toggle: Retained<NSButton>,
    learn: Retained<NSButton>,
    status: Retained<NSTextField>,
}

impl VoiceBar {
    /// A bar filling `frame`, pinned to the top of its superview.
    pub fn new(mtm: MainThreadMarker, actions: &ActionSink<SlackAction>, frame: NSRect) -> Self {
        let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        // A card of its own, controls on one row: toggle, status, then relearn.
        widgets::paint(&view, Some(&theme::card()), None);
        let top = (frame.size.height - BUTTON_HEIGHT) / 2.0;
        let width = frame.size.width;

        let toggle = widgets::text_button(
            mtm,
            "□ Write like me",
            widgets::ButtonStyle::Quiet,
            &actions.target(SlackAction::ToggleWriteAsMe),
            Action::selector(),
        );
        toggle.setFrame(rect(PADDING, top, TOGGLE_WIDTH, BUTTON_HEIGHT));
        view.addSubview(&toggle);

        let learn = widgets::text_button(
            mtm,
            "Learn",
            widgets::ButtonStyle::Quiet,
            &actions.target(SlackAction::LearnMyStyle),
            Action::selector(),
        );
        learn.setFrame(rect(
            width - PADDING - LEARN_WIDTH,
            top,
            LEARN_WIDTH,
            BUTTON_HEIGHT,
        ));
        learn.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        view.addSubview(&learn);

        let status = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        let status_x = PADDING + TOGGLE_WIDTH + GAP;
        status.setFrame(rect(
            status_x,
            (frame.size.height - 18.0) / 2.0,
            width - status_x - GAP - LEARN_WIDTH - PADDING,
            18.0,
        ));
        status.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        view.addSubview(&status);

        Self {
            view,
            toggle,
            learn,
            status,
        }
    }

    pub fn render(&self, state: &VoiceBarState) {
        // Green while on, like a lit switch.
        let (toggle, style) = if state.enabled {
            ("■ Write like me", widgets::ButtonStyle::Go)
        } else {
            ("□ Write like me", widgets::ButtonStyle::Quiet)
        };
        widgets::restyle_button(&self.toggle, toggle, style);
        let learn = if state.learned { "Relearn" } else { "Learn" };
        widgets::set_text_button_label(&self.learn, learn, widgets::ButtonStyle::Quiet);
        self.learn.setEnabled(!state.busy);
        self.status.setAttributedStringValue(&rich_text::attributed(
            state.status,
            &Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE)),
        ));
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
