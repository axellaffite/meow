//! The Autopilot page: work Meow started by itself from Slack messages, newest
//! first. Each card says who asked what, how the work went, and lists the
//! actions that wait for the user (a push, a reply…) with Approve and Reject.

use std::cell::RefCell;

use meow::autopilot::{self, Action, ActionState, Job, Jobs, Stage};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSScrollView, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSAttributedString, NSPoint, NSRect, NSSize, NSString};

use super::bubble;
use super::controller::Controller;
use super::flipped::FlippedView;
use super::rich_text::{self, Attributes};
use super::theme;
use super::widgets;

const MARGIN: f64 = 16.0;
const ROW_HEIGHT: f64 = 22.0;
const CARD_PADDING: f64 = 8.0;
const CARD_GAP: f64 = 6.0;
const LINE_GAP: f64 = 4.0;
const STATUS_WIDTH: f64 = 150.0;
const DELETE_WIDTH: f64 = 22.0;
const BUTTON_WIDTH: f64 = 82.0;
const BUTTON_GAP: f64 = 6.0;
const FOLDER_WIDTH: f64 = 110.0;
const TOGGLE_WIDTH: f64 = 110.0;
const INDENT: f64 = 6.0;
const ACTION_INSET: f64 = 14.0;
const WAITING_BORDER: f64 = 2.0;
/// Longest a held call's details are shown.
const MAX_DETAIL_CHARS: usize = 700;
/// Action buttons carry `job row * TAG_STRIDE + action index`.
pub const TAG_STRIDE: usize = 1000;

pub struct AutopilotPage {
    pub view: Retained<NSView>,
    scroll: Retained<NSScrollView>,
    list: Retained<FlippedView>,
    target: Retained<Controller>,
    /// Job ids in the order shown; buttons carry an index into it.
    shown_ids: RefCell<Vec<u64>>,
    mtm: MainThreadMarker,
}

impl AutopilotPage {
    pub fn new(mtm: MainThreadMarker, controller: &Retained<Controller>, frame: NSRect) -> Self {
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
            target: controller.clone(),
            shown_ids: RefCell::new(Vec::new()),
            mtm,
        }
    }

    /// The id of the job in row `index`, as last rendered.
    pub fn id_at(&self, index: usize) -> Option<u64> {
        self.shown_ids.borrow().get(index).copied()
    }

    /// Draws the switch, then the jobs.
    pub fn render(&self, jobs: &Jobs, enabled: bool, now: u64) {
        let width = self.scroll.contentSize().width;
        for row in self.list.subviews().iter() {
            row.removeFromSuperview();
        }
        let mut y = self.add_switch(enabled, width);
        let mut ids = Vec::new();
        for job in jobs.all() {
            y = self.add_job(ids.len(), job, width, y, now) + CARD_GAP;
            ids.push(job.id);
        }
        let height = y.max(self.scroll.contentSize().height);
        self.list.setFrameSize(NSSize::new(width, height));
        self.shown_ids.replace(ids);
    }

    /// What Autopilot does, and the button turning it on or off. Returns the y below.
    fn add_switch(&self, enabled: bool, width: f64) -> f64 {
        let target: &AnyObject = &self.target;
        let hint = if enabled {
            "On: when a Slack message asks you for something, Claude prepares it here. \
             Nothing that can't be undone (a push, a message sent, an issue changed) \
             happens until you approve it."
        } else {
            "Off. Turned on, Claude prepares what Slack messages ask you for (fixing a \
             PR's conflicts, drafting a reply…), and waits for your approval before \
             anything that can't be undone."
        };
        let style = Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE));
        let text_width = width - TOGGLE_WIDTH - 3.0 * INDENT;
        let height = self
            .add_wrapped(
                &rich_text::attributed(hint, &style),
                INDENT,
                0.0,
                text_width,
            )
            .max(ROW_HEIGHT);
        let (label, style) = if enabled {
            ("Turn off", widgets::ButtonStyle::Quiet)
        } else {
            ("Turn on", widgets::ButtonStyle::Go)
        };
        let toggle = widgets::text_button(self.mtm, label, style, target, sel!(toggleAutopilot:));
        toggle.setFrame(rect(
            width - INDENT - TOGGLE_WIDTH,
            0.0,
            TOGGLE_WIDTH,
            ROW_HEIGHT,
        ));
        toggle.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        self.list.addSubview(&toggle);
        height + 2.0 * CARD_GAP
    }

    /// A card: title, status and ×; who asked what; the work's summary; then
    /// each held action. Returns the y below.
    fn add_job(&self, row: usize, job: &Job, width: f64, y: f64, now: u64) -> f64 {
        let target: &AnyObject = &self.target;
        let card = FlippedView::new(self.mtm, rect(0.0, y, width, 0.0));
        card.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        self.list.addSubview(&card);
        let inner = width - 4.0 * INDENT;
        let mut top = y + CARD_PADDING;

        let delete_x = width - INDENT - DELETE_WIDTH;
        let status_x = delete_x - STATUS_WIDTH;
        let title = Attributes {
            heavy: true,
            ..Attributes::plain(theme::ink(), theme::font(theme::BODY_SIZE))
        };
        self.add_label(
            &rich_text::attributed(&job.title, &title),
            rect(2.0 * INDENT, top, status_x - 3.0 * INDENT, ROW_HEIGHT),
        );
        let (status, color) = status(job, now);
        let small = Attributes::plain(color, theme::font(theme::SMALL_SIZE));
        let label = self.add_label(
            &rich_text::attributed(&status, &small),
            rect(status_x, top + 3.0, STATUS_WIDTH, ROW_HEIGHT - 3.0),
        );
        label.setAlignment(NSTextAlignment::Right);
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        let delete = widgets::text_button(
            self.mtm,
            "×",
            widgets::ButtonStyle::Link,
            target,
            sel!(dismissAutopilot:),
        );
        delete.setTag(row as isize);
        delete.setToolTip(Some(&NSString::from_str(
            "Remove this job, and its worktree",
        )));
        delete.setFrame(rect(delete_x, top, DELETE_WIDTH, ROW_HEIGHT));
        delete.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        self.list.addSubview(&delete);
        top += ROW_HEIGHT;

        let muted = Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE));
        let asked = format!("{}: {}", job.origin.from, clip(&job.origin.text, 240));
        top += self.add_wrapped(
            &rich_text::attributed(&asked, &muted),
            2.0 * INDENT,
            top,
            inner,
        ) + LINE_GAP;

        if let Some(folder) = &job.folder {
            let faint = Attributes::plain(theme::faint(), theme::font(theme::SMALL_SIZE));
            let line = format!("{} · {}", folder.branch, folder.path.display());
            let text_width = inner - FOLDER_WIDTH - INDENT;
            let label = self.add_label(
                &rich_text::attributed(&line, &faint),
                rect(2.0 * INDENT, top + 3.0, text_width, ROW_HEIGHT - 3.0),
            );
            label.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingMiddle);
            let open = widgets::text_button(
                self.mtm,
                "Open folder",
                widgets::ButtonStyle::Link,
                target,
                sel!(openAutopilotFolder:),
            );
            open.setTag(row as isize);
            open.setFrame(rect(
                width - 2.0 * INDENT - FOLDER_WIDTH,
                top,
                FOLDER_WIDTH,
                ROW_HEIGHT,
            ));
            open.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
            self.list.addSubview(&open);
            top += ROW_HEIGHT + LINE_GAP;
        }

        if let Some(summary) = &job.summary {
            let color = if job.stage == Stage::Failed {
                theme::error()
            } else {
                theme::ink()
            };
            let body = Attributes::plain(color, theme::font(theme::SMALL_SIZE));
            top += self.add_wrapped(
                &rich_text::attributed(summary, &body),
                2.0 * INDENT,
                top,
                inner,
            ) + LINE_GAP;
        }

        for (index, action) in job.actions.iter().enumerate() {
            top = self.add_action(row * TAG_STRIDE + index, action, width, top) + LINE_GAP;
        }

        let height = top - y + CARD_PADDING - LINE_GAP;
        card.setFrameSize(NSSize::new(width, height));
        let fill = if job.is_open() {
            theme::card()
        } else {
            theme::card_done()
        };
        let border = (job.pending() > 0).then(theme::accent);
        widgets::paint(
            &card,
            Some(&fill),
            border.as_deref().map(|color| (color, WAITING_BORDER)),
        );
        y + height
    }

    /// A held action: what it does and its details, with Approve and Reject
    /// while it waits, else how it went. Returns the y below.
    fn add_action(&self, tag: usize, action: &Action, width: f64, y: f64) -> f64 {
        let target: &AnyObject = &self.target;
        let (what, detail) = autopilot::describe(&action.call);
        let x = 2.0 * INDENT + ACTION_INSET;
        let buttons_width = 2.0 * BUTTON_WIDTH + BUTTON_GAP;
        let text_width = width - x - 2.0 * INDENT - buttons_width - INDENT;
        let heading = Attributes {
            heavy: true,
            ..Attributes::plain(theme::highlight(), theme::font(theme::SMALL_SIZE))
        };
        let mut top = y;
        top += self.add_wrapped(&rich_text::attributed(&what, &heading), x, top, text_width);
        let code = Attributes::plain(theme::ink(), theme::font(theme::SMALL_SIZE));
        top += self.add_wrapped(
            &rich_text::attributed(&clip(&detail, MAX_DETAIL_CHARS), &code),
            x,
            top,
            text_width,
        );
        let right = width - 2.0 * INDENT;
        let pin = NSAutoresizingMaskOptions::ViewMinXMargin;
        match action.state {
            ActionState::Pending => {
                let buttons = [
                    ("Approve", widgets::ButtonStyle::Go, sel!(approveAutopilot:)),
                    (
                        "Reject",
                        widgets::ButtonStyle::Quiet,
                        sel!(rejectAutopilot:),
                    ),
                ];
                let mut x = right - buttons_width;
                for (label, style, selector) in buttons {
                    let button = widgets::text_button(self.mtm, label, style, target, selector);
                    button.setTag(tag as isize);
                    button.setFrame(rect(x, y, BUTTON_WIDTH, ROW_HEIGHT));
                    button.setAutoresizingMask(pin);
                    self.list.addSubview(&button);
                    x += BUTTON_WIDTH + BUTTON_GAP;
                }
            }
            state => {
                let (text, color) = action_status(state);
                let style = Attributes::plain(color, theme::font(theme::SMALL_SIZE));
                let label = self.add_label(
                    &rich_text::attributed(text, &style),
                    rect(
                        right - buttons_width,
                        y + 3.0,
                        buttons_width,
                        ROW_HEIGHT - 3.0,
                    ),
                );
                label.setAlignment(NSTextAlignment::Right);
                label.setAutoresizingMask(pin);
                if let Some(result) = &action.result {
                    let muted = Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE));
                    top += self.add_wrapped(
                        &rich_text::attributed(result, &muted),
                        x,
                        top,
                        text_width,
                    );
                }
            }
        }
        top.max(y + ROW_HEIGHT)
    }

    fn add_label(&self, text: &NSAttributedString, frame: NSRect) -> Retained<NSTextField> {
        let label = NSTextField::labelWithAttributedString(text, self.mtm);
        label.setFrame(frame);
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        self.list.addSubview(&label);
        label
    }

    /// A label wrapped to `width`; returns its height.
    fn add_wrapped(&self, text: &NSAttributedString, x: f64, y: f64, width: f64) -> f64 {
        let label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), self.mtm);
        label.setAttributedStringValue(text);
        label.setPreferredMaxLayoutWidth(width);
        let height = label
            .sizeThatFits(NSSize::new(width, f64::MAX))
            .height
            .ceil();
        label.setFrame(rect(x, y, width, height));
        self.list.addSubview(&label);
        height
    }
}

/// The job's status at the top right of its card.
fn status(job: &Job, now: u64) -> (String, Retained<NSColor>) {
    match job.stage {
        Stage::Queued => ("waiting its turn".into(), theme::muted()),
        Stage::Preparing => ("making a worktree…".into(), theme::live()),
        Stage::Working => ("working…".into(), theme::live()),
        Stage::Failed => ("failed".into(), theme::error()),
        Stage::Finished => match job.pending() {
            0 => {
                let ago = job
                    .finished_at
                    .map(|at| now.saturating_sub(at) / 60)
                    .map(|minutes| match minutes {
                        0 => "done just now".to_string(),
                        m if m < 60 => format!("done {m} min ago"),
                        m => format!("done {} h ago", m / 60),
                    })
                    .unwrap_or_else(|| "done".into());
                (ago, theme::faint())
            }
            1 => ("1 to approve".into(), theme::accent()),
            n => (format!("{n} to approve"), theme::accent()),
        },
    }
}

fn action_status(state: ActionState) -> (&'static str, Retained<NSColor>) {
    match state {
        ActionState::Pending => ("", theme::muted()),
        ActionState::Approved => ("approved, next up", theme::live()),
        ActionState::Running => ("making it…", theme::live()),
        ActionState::Done => ("done", theme::live()),
        ActionState::Rejected => ("rejected", theme::faint()),
        ActionState::Failed => ("failed", theme::error()),
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max).collect();
    clipped.push('…');
    clipped
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
