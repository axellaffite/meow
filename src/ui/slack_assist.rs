//! The private Claude panel of a Slack conversation: the user's question,
//! Claude's answer and, when there is one, a draft to put in the reply box,
//! a calendar event waiting for the user's go, or a coding agent Claude
//! proposes and, once started, what it writes. Nothing here is ever posted;
//! it sits between the transcript and the input.

use meow::slack::format::{self, NameMap};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSButton, NSScrollView, NSTextView, NSView};
use objc2_foundation::{NSAttributedString, NSMutableAttributedString, NSPoint, NSRect, NSSize};

use super::action::{Action, ActionSink};
use super::controller::Controller;
use super::rich_text::{self, Attributes};
use super::slack_page::SlackAction;
use super::theme;
use super::widgets;

pub const HEIGHT: f64 = 220.0;
const BUTTON_HEIGHT: f64 = 28.0;
const USE_WIDTH: f64 = 136.0;
const CREATE_WIDTH: f64 = 176.0;
const START_WIDTH: f64 = 156.0;
const CLOSE_WIDTH: f64 = 84.0;
const GAP: f64 = 6.0;
/// Width of the gold frame.
const FRAME: f64 = 3.0;
/// Space inside the frame.
const INSET: f64 = FRAME + 10.0;
/// The latest steps of a research run that stay listed while it works.
const MAX_STEPS_SHOWN: usize = 6;

/// What the panel shows.
pub enum AssistView<'a> {
    Thinking {
        question: &'a str,
        /// What is under way ("Reading the conversation…").
        label: &'a str,
        /// What Claude has looked at so far (tool calls), oldest first.
        steps: &'a [String],
    },
    Answer {
        question: &'a str,
        answer: &'a str,
        draft: Option<&'a str>,
    },
    Failed {
        question: &'a str,
        error: &'a str,
    },
    /// A calendar event Claude proposes, created only on the user's go.
    Event {
        question: &'a str,
        note: &'a str,
        summary: &'a str,
        when: &'a str,
        attendees: &'a [String],
        /// The creation is under way.
        creating: bool,
    },
    /// A coding agent Claude proposes, started only on the user's go.
    AgentProposal {
        question: &'a str,
        note: &'a str,
        project: &'a str,
        prompt: &'a str,
    },
    /// A started agent: what happened so far and its latest message.
    Agent {
        question: &'a str,
        /// "project · workspace".
        title: &'a str,
        steps: &'a [String],
        status: &'a str,
        message: Option<&'a str>,
    },
}

pub struct AssistPanel {
    pub view: Retained<NSView>,
    text: Retained<NSTextView>,
    use_draft: Retained<NSButton>,
    create: Retained<NSButton>,
    start_agent: Retained<NSButton>,
}

impl AssistPanel {
    /// A hidden panel filling `frame`, its width following its superview.
    pub fn new(
        mtm: MainThreadMarker,
        controller: &Retained<Controller>,
        actions: &ActionSink<SlackAction>,
        frame: NSRect,
    ) -> Self {
        let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        let width = frame.size.width;
        // A gold pixel frame: this is Claude talking to the user alone.
        widgets::paint(
            &view,
            Some(&theme::sidebar()),
            Some((&theme::highlight(), FRAME)),
        );

        let close = widgets::text_button(
            mtm,
            "Close",
            widgets::ButtonStyle::Quiet,
            &actions.target(SlackAction::CloseAssist),
            Action::selector(),
        );
        close.setFrame(rect(
            width - INSET - CLOSE_WIDTH,
            INSET,
            CLOSE_WIDTH,
            BUTTON_HEIGHT,
        ));
        close.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        view.addSubview(&close);

        let use_draft = widgets::text_button(
            mtm,
            "Use draft",
            widgets::ButtonStyle::Primary,
            &actions.target(SlackAction::UseDraft),
            Action::selector(),
        );
        use_draft.setFrame(rect(
            width - INSET - CLOSE_WIDTH - GAP - USE_WIDTH,
            INSET,
            USE_WIDTH,
            BUTTON_HEIGHT,
        ));
        use_draft.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        view.addSubview(&use_draft);

        let create = widgets::text_button(
            mtm,
            "Create event",
            widgets::ButtonStyle::Go,
            &actions.target(SlackAction::CreateEvent),
            Action::selector(),
        );
        create.setFrame(rect(
            width - INSET - CLOSE_WIDTH - GAP - CREATE_WIDTH,
            INSET,
            CREATE_WIDTH,
            BUTTON_HEIGHT,
        ));
        create.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        view.addSubview(&create);

        let start_agent = widgets::text_button(
            mtm,
            "Start agent",
            widgets::ButtonStyle::Go,
            &actions.target(SlackAction::StartAgent),
            Action::selector(),
        );
        start_agent.setFrame(rect(
            width - INSET - CLOSE_WIDTH - GAP - START_WIDTH,
            INSET,
            START_WIDTH,
            BUTTON_HEIGHT,
        ));
        start_agent.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        view.addSubview(&start_agent);

        let scroll: Retained<NSScrollView> = NSTextView::scrollableTextView(mtm);
        scroll.setFrame(rect(
            FRAME,
            INSET + BUTTON_HEIGHT + GAP,
            width - 2.0 * FRAME,
            frame.size.height - INSET - BUTTON_HEIGHT - GAP - FRAME,
        ));
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        scroll.setDrawsBackground(false);
        let text = scroll
            .documentView()
            .and_then(|view| view.downcast::<NSTextView>().ok())
            .expect("scrollable text view contains a text view");
        text.setEditable(false);
        text.setDrawsBackground(false);
        text.setTextContainerInset(NSSize::new(INSET, INSET));
        rich_text::style_links(&text);
        text.setDelegate(Some(ProtocolObject::from_ref(&**controller)));
        view.addSubview(&scroll);

        view.setHidden(true);
        Self {
            view,
            text,
            use_draft,
            create,
            start_agent,
        }
    }

    pub fn is_shown(&self) -> bool {
        !self.view.isHidden()
    }

    pub fn hide(&self) {
        self.view.setHidden(true);
    }

    pub fn show(&self, content: &AssistView) {
        self.view.setHidden(false);
        let text = NSMutableAttributedString::new();
        let small = |s: &str| {
            rich_text::attributed(
                s,
                &Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE)),
            )
        };
        text.appendAttributedString(&rich_text::attributed(
            "PRIVATE · NOTHING IS POSTED\n",
            &Attributes::plain(theme::highlight(), theme::label_font(theme::LABEL_SIZE)),
        ));
        let mut can_create = false;
        let mut can_start = false;
        let has_draft = match content {
            AssistView::Thinking {
                question,
                label,
                steps,
            } => {
                text.appendAttributedString(&small(&format!("You: « {question} »\n\n")));
                text.appendAttributedString(&small(&format!("{label}\n")));
                let shown = steps.len().saturating_sub(MAX_STEPS_SHOWN);
                for step in &steps[shown..] {
                    text.appendAttributedString(&small(&format!("    - {step}\n")));
                }
                false
            }
            AssistView::Answer {
                question,
                answer,
                draft,
            } => {
                text.appendAttributedString(&small(&format!("You: « {question} »\n\n")));
                text.appendAttributedString(&rich_text::slack_assist_body(answer));
                if let Some(draft) = draft {
                    text.appendAttributedString(&rich_text::attributed(
                        "\nDRAFT\n",
                        &Attributes::plain(theme::muted(), theme::label_font(theme::LABEL_SIZE)),
                    ));
                    // The draft is Slack `mrkdwn`, as it will be sent.
                    let shown = format::to_markdown(draft, &NameMap::default());
                    text.appendAttributedString(&rich_text::slack_assist_body(&shown));
                }
                draft.is_some()
            }
            AssistView::Failed { question, error } => {
                text.appendAttributedString(&small(&format!("You: « {question} »\n\n")));
                text.appendAttributedString(&rich_text::attributed(
                    error,
                    &Attributes::plain(theme::error(), theme::font(theme::BODY_SIZE)),
                ));
                false
            }
            AssistView::Event {
                question,
                note,
                summary,
                when,
                attendees,
                creating,
            } => {
                text.appendAttributedString(&small(&format!("You: « {question} »\n\n")));
                if !note.is_empty() {
                    text.appendAttributedString(&small(&format!("{note}\n\n")));
                }
                text.appendAttributedString(&rich_text::attributed(
                    &format!("{summary}\n"),
                    &Attributes {
                        heavy: true,
                        ..Attributes::plain(theme::ink(), theme::font(theme::BODY_SIZE))
                    },
                ));
                let body = format!("When: {when}\nWith: {}\n", attendees.join(", "));
                text.appendAttributedString(&rich_text::attributed(
                    &body,
                    &Attributes::plain(theme::ink(), theme::font(theme::BODY_SIZE)),
                ));
                if *creating {
                    text.appendAttributedString(&small("\nCreating the event…\n"));
                } else {
                    text.appendAttributedString(&small(
                        "\nNothing is created until you click Create event.\n",
                    ));
                }
                can_create = !creating;
                false
            }
            AssistView::AgentProposal {
                question,
                note,
                project,
                prompt,
            } => {
                text.appendAttributedString(&small(&format!("You: « {question} »\n\n")));
                if !note.is_empty() {
                    text.appendAttributedString(&small(&format!("{note}\n\n")));
                }
                text.appendAttributedString(&heading(&format!("Agent in {project}\n")));
                text.appendAttributedString(&rich_text::slack_assist_body(prompt));
                text.appendAttributedString(&small(
                    "\n\nNothing starts until you click Start agent: it gets a new worktree \
                     in Superset and runs Claude Code there.\n",
                ));
                can_start = true;
                false
            }
            AssistView::Agent {
                question,
                title,
                steps,
                status,
                message,
            } => {
                text.appendAttributedString(&small(&format!("You: « {question} »\n\n")));
                text.appendAttributedString(&heading(&format!("Agent · {title}\n")));
                for step in steps.iter() {
                    text.appendAttributedString(&small(&format!("    - {step}\n")));
                }
                text.appendAttributedString(&small(&format!("\n{status}\n\n")));
                if let Some(message) = message {
                    text.appendAttributedString(&rich_text::slack_assist_body(message));
                }
                false
            }
        };
        self.use_draft.setHidden(!has_draft);
        self.create.setHidden(!can_create);
        self.start_agent.setHidden(!can_start);
        // SAFETY: the text view always owns a text storage.
        if let Some(storage) = unsafe { self.text.textStorage() } {
            storage.setAttributedString(&text);
        }
        self.text
            .scrollRangeToVisible(objc2_foundation::NSRange::new(0, 0));
    }
}

fn heading(text: &str) -> Retained<NSAttributedString> {
    rich_text::attributed(
        text,
        &Attributes {
            heavy: true,
            ..Attributes::plain(theme::ink(), theme::font(theme::BODY_SIZE))
        },
    )
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
