//! The Slack page: a list of conversations, a message view with a reply box,
//! and a signed-out state offering to sign in.
//!
//! Like the other pages it is pure view. The app fills it with
//! [`SlackPage::show_signed_out`], [`SlackPage::render_list`] and
//! [`SlackPage::render_conversation`], and reads back the row index of a click
//! ([`SlackPage::id_at`]) and the typed reply ([`SlackPage::take_input`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;

use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAttachmentAttributeName, NSAutoresizingMaskOptions, NSButton, NSScrollView, NSTextAlignment,
    NSTextAttachment, NSTextField, NSTextView, NSView, NSWindow,
};
use objc2_foundation::{
    NSAttributedString, NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString,
};

use super::action::{Action, ActionSink, LinkDelegate};
use super::bubble;
use super::controller::Controller;
use super::emoji_text::{self, Animated, EmojiGlyph, ImageGlyph};
use super::flipped::FlippedView;
use super::link_button::LinkButton;
use super::rich_text::{self, Attributes};
use super::slack_assist::{self, AssistPanel, AssistView};
use super::slack_voice_bar::{self, VoiceBar, VoiceBarState};
use super::theme;
use super::widgets;

const MARGIN: f64 = 10.0;
const INPUT_HEIGHT: f64 = 28.0;
const LIVE_WIDTH: f64 = 80.0;
const SEND_WIDTH: f64 = 76.0;
const ASK_WIDTH: f64 = 64.0;
const AUTO_WIDTH: f64 = 68.0;
const AGENT_WIDTH: f64 = 76.0;
const BACK_WIDTH: f64 = 110.0;
const TOP_HEIGHT: f64 = 40.0;
const ROW_HEIGHT: f64 = 30.0;
const INDENT: f64 = 6.0;
/// Room on the right of a row for its mention count and time.
const META_WIDTH: f64 = 120.0;
/// Each conversation of the list sits on a square card.
const CARD_HEIGHT: f64 = 44.0;
const CARD_GAP: f64 = 6.0;
const CARD_PADDING: f64 = 12.0;
/// The Direct and Channels tabs above the list.
const TAB_WIDTH: f64 = 150.0;
const TAB_HEIGHT: f64 = 28.0;
const TAB_GAP: f64 = 6.0;
const BACK_TO_LIST: &str = "‹ Messages";
const BACK_TO_CONVERSATION: &str = "‹ Back";
const REPLY_HINT: &str = "Reply, or ask Claude… (Esc: back)";
const THREAD_REPLY_HINT: &str = "Reply in thread, or ask Claude… (Esc: back)";

/// The Slack page's id among the features' pages.
pub const PAGE_ID: &str = "slack";

/// What the Slack page's controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlackAction {
    SignIn,
    /// Show the list's tab of direct messages, or of channels.
    Tab(SlackTab),
    /// Open the conversation in this row, as last listed.
    Open(usize),
    /// Open the thread of the open conversation's message at this index.
    OpenThread(usize),
    /// Back from a thread to its conversation, or from a conversation to the list.
    Back,
    /// Send the reply typed in the open conversation.
    Send,
    /// Ask Claude, privately, about the open conversation.
    Ask,
    /// Have Claude research and draft the reply, with the user's tools.
    Autocomplete,
    /// Have Claude propose a coding agent for the open conversation.
    Agent,
    /// Turn drafting in the user's own style on or off.
    ToggleWriteAsMe,
    /// Learn the user's style from their Slack messages.
    LearnMyStyle,
    /// Put Claude's draft in the reply box.
    UseDraft,
    /// Create the calendar event Claude proposed.
    CreateEvent,
    /// Close the private Claude panel.
    CloseAssist,
    /// Start the coding agent Claude proposed.
    StartAgent,
}

/// Which conversations the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlackTab {
    /// Direct and group messages.
    #[default]
    Direct,
    Channels,
}

/// One conversation in the list.
pub struct ConversationRow {
    pub id: String,
    pub title: String,
    pub unread: bool,
    /// Unread mentions of the user.
    pub mentions: u32,
    /// When it was last active ("14:32", "Mon"); may be empty.
    pub time: String,
    /// A direct or group message, rather than a channel.
    pub direct: bool,
}

/// A piece of the open conversation, names resolved and bodies in Markdown.
pub enum TranscriptItem {
    Day(String),
    /// Consecutive messages by one author, under a single header.
    Run {
        author: String,
        time: String,
        mine: bool,
        bodies: Vec<String>,
        /// The body to mark and scroll to, if any.
        focus: Option<usize>,
    },
}

/// An animated picture in the transcript: where it is, and the frame shown.
struct Playing {
    animated: Animated,
    range: NSRange,
    frame: usize,
}

pub struct SlackPage {
    pub view: Retained<NSView>,
    /// Shown signed out: a prompt and a sign-in button.
    signin: Retained<NSButton>,
    status: Retained<NSTextField>,
    /// Conversation list (list mode).
    list_scroll: Retained<NSScrollView>,
    list: Retained<FlippedView>,
    /// Message view and reply box (conversation mode).
    convo_scroll: Retained<NSScrollView>,
    transcript: Retained<NSTextView>,
    input: Retained<widgets::GrowingField>,
    send: Retained<NSButton>,
    /// Animated emoji and images in the transcript, and when they started.
    playing: RefCell<Vec<Playing>>,
    clock: Instant,
    /// Asks Claude privately about the conversation, with the typed question.
    ask: Retained<NSButton>,
    /// Has Claude research and draft the reply, with the user's tools.
    auto: Retained<NSButton>,
    /// Has Claude propose a coding agent for the conversation.
    agent: Retained<NSButton>,
    /// "Write like me" controls, above the list.
    voice: VoiceBar,
    assist: AssistPanel,
    back: Retained<NSButton>,
    convo_title: Retained<NSTextField>,
    /// The dark strip holding the back link, title and live mark.
    strip: Retained<NSView>,
    /// The reply bar of the conversation.
    composer: Retained<NSView>,
    /// Conversation ids in the order shown; row buttons carry an index into it.
    shown_ids: RefCell<Vec<String>>,
    tab: Cell<SlackTab>,
    /// The targets of the page's fixed controls, kept for them.
    _actions: ActionSink<SlackAction>,
    /// Opens threads from the transcript's reply counts; kept, as the
    /// transcript holds its delegate weakly.
    _links: Retained<LinkDelegate>,
    /// The targets of the list's tabs and rows, dropped as they are redrawn.
    row_actions: ActionSink<SlackAction>,
    mtm: MainThreadMarker,
}

impl SlackPage {
    /// The page, its controls reporting to `on_action`; `controller` handles
    /// the text fields' keys and links.
    pub fn new(
        mtm: MainThreadMarker,
        controller: &Retained<Controller>,
        frame: NSRect,
        on_action: impl Fn(SlackAction) + 'static,
    ) -> Self {
        let actions = ActionSink::new(mtm, on_action);
        let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        // The page runs under the bubble's right padding; content keeps clear of it.
        let (width, height) = (frame.size.width - bubble::PADDING, frame.size.height);

        // --- Signed-out state ---------------------------------------------
        let signin = widgets::text_button(
            mtm,
            "Sign in to Slack",
            widgets::ButtonStyle::Primary,
            &actions.target(SlackAction::SignIn),
            Action::selector(),
        );
        signin.setFrame(rect(
            MARGIN,
            height / 2.0,
            width - 2.0 * MARGIN,
            INPUT_HEIGHT,
        ));
        signin.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        view.addSubview(&signin);

        let status = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        status.setFrame(rect(
            MARGIN,
            height / 2.0 - ROW_HEIGHT,
            width - 2.0 * MARGIN,
            ROW_HEIGHT,
        ));
        status.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        view.addSubview(&status);

        // --- List mode ----------------------------------------------------
        let list_scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            rect(
                MARGIN,
                MARGIN,
                width - 2.0 * MARGIN,
                height - 3.0 * MARGIN - slack_voice_bar::HEIGHT,
            ),
        );
        list_scroll.setHasVerticalScroller(true);
        list_scroll.setDrawsBackground(false);
        list_scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let list = FlippedView::new(mtm, rect(0.0, 0.0, list_scroll.contentSize().width, 0.0));
        list.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        list_scroll.setDocumentView(Some(&list));
        view.addSubview(&list_scroll);

        let voice = VoiceBar::new(
            mtm,
            &actions,
            rect(
                MARGIN,
                height - MARGIN - slack_voice_bar::HEIGHT,
                width - 2.0 * MARGIN,
                slack_voice_bar::HEIGHT,
            ),
        );
        view.addSubview(&voice.view);

        // --- Conversation mode --------------------------------------------
        // The strip and the reply bar live in a view of their own, so they
        // hide with the conversation and can run on to the bubble's outline.
        let composer =
            NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, frame.size.width, height));
        composer.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        view.addSubview(&composer);
        // A dark strip on top: back link, who it is with, and the live mark.
        let strip = NSView::initWithFrame(
            NSView::alloc(mtm),
            rect(0.0, height - TOP_HEIGHT, frame.size.width, TOP_HEIGHT),
        );
        strip.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        widgets::paint(&strip, Some(&theme::sidebar()), None);
        composer.addSubview(&strip);

        let back = widgets::text_button(
            mtm,
            BACK_TO_LIST,
            widgets::ButtonStyle::Link,
            &actions.target(SlackAction::Back),
            Action::selector(),
        );
        back.setFrame(rect(INDENT, 0.0, BACK_WIDTH, TOP_HEIGHT));
        strip.addSubview(&back);

        let convo_title = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        convo_title.setFrame(rect(
            INDENT + BACK_WIDTH,
            (TOP_HEIGHT - 22.0) / 2.0,
            width - 2.0 * INDENT - BACK_WIDTH - LIVE_WIDTH,
            22.0,
        ));
        convo_title.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        strip.addSubview(&convo_title);

        let live = NSTextField::labelWithAttributedString(
            &rich_text::attributed(
                "■ LIVE",
                &Attributes::plain(theme::live(), theme::label_font(theme::LABEL_SIZE)),
            ),
            mtm,
        );
        live.setAlignment(NSTextAlignment::Right);
        live.setFrame(rect(
            width - MARGIN - LIVE_WIDTH,
            (TOP_HEIGHT - 18.0) / 2.0,
            LIVE_WIDTH,
            18.0,
        ));
        live.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        strip.addSubview(&live);

        let input = widgets::text_field(mtm, REPLY_HINT);
        let send_target = actions.target(SlackAction::Send);
        // SAFETY: the page keeps the target, and the controller lives as long
        // as the app, so the weak target and delegate stay valid.
        unsafe {
            input.setTarget(Some(&send_target));
            input.setAction(Some(Action::selector()));
            input.setDelegate(Some(ProtocolObject::from_ref(&**controller)));
        }
        // Send on Return only. By default the field also fires its action when
        // editing ends (focus moving, a click elsewhere), which would post a
        // half-written or freshly drafted message.
        if let Some(cell) = input.cell() {
            cell.setSendsActionOnEndEditing(false);
        }

        let send = widgets::text_button(
            mtm,
            "Send",
            widgets::ButtonStyle::Go,
            &send_target,
            Action::selector(),
        );
        let ask = widgets::text_button(
            mtm,
            "Ask",
            widgets::ButtonStyle::Accent,
            &actions.target(SlackAction::Ask),
            Action::selector(),
        );
        let auto = widgets::text_button(
            mtm,
            "Auto",
            widgets::ButtonStyle::OutlineAccent,
            &actions.target(SlackAction::Autocomplete),
            Action::selector(),
        );
        let agent = widgets::text_button(
            mtm,
            "Agent",
            widgets::ButtonStyle::OutlineLink,
            &actions.target(SlackAction::Agent),
            Action::selector(),
        );
        agent.setToolTip(Some(&NSString::from_str(
            "Start a coding agent in Superset for this conversation",
        )));
        widgets::composer(
            mtm,
            &composer,
            &input,
            &[
                (&ask, ASK_WIDTH, widgets::ButtonStyle::Accent),
                (&auto, AUTO_WIDTH, widgets::ButtonStyle::OutlineAccent),
                (&agent, AGENT_WIDTH, widgets::ButtonStyle::OutlineLink),
            ],
            &[(&send, SEND_WIDTH, widgets::ButtonStyle::Go)],
        );

        let assist = AssistPanel::new(
            mtm,
            controller,
            &actions,
            rect(
                MARGIN,
                widgets::STACKED_COMPOSER_HEIGHT + MARGIN,
                width - 2.0 * MARGIN,
                slack_assist::HEIGHT,
            ),
        );
        view.addSubview(&assist.view);
        input.keep_above(&assist.view);

        let convo_scroll = NSTextView::scrollableTextView(mtm);
        let top = widgets::STACKED_COMPOSER_HEIGHT + MARGIN;
        convo_scroll.setFrame(rect(
            MARGIN,
            top,
            width - 2.0 * MARGIN,
            height - top - TOP_HEIGHT,
        ));
        convo_scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        convo_scroll.setDrawsBackground(false);
        let transcript = convo_scroll
            .documentView()
            .and_then(|view| view.downcast::<NSTextView>().ok())
            .expect("scrollable text view contains a text view");
        transcript.setEditable(false);
        transcript.setDrawsBackground(false);
        transcript.setTextContainerInset(NSSize::new(4.0, 6.0));
        rich_text::style_links(&transcript);
        let open_thread = actions.sender();
        let links = LinkDelegate::new(mtm, move |url| match rich_text::thread_index(url) {
            Some(index) => {
                open_thread(SlackAction::OpenThread(index));
                true
            }
            None => false,
        });
        transcript.setDelegate(Some(ProtocolObject::from_ref(&*links)));
        view.addSubview(&convo_scroll);
        input.keep_clear(&convo_scroll);

        let page = Self {
            view,
            signin,
            status,
            list_scroll,
            list,
            convo_scroll,
            transcript,
            playing: RefCell::new(Vec::new()),
            clock: Instant::now(),
            input,
            send,
            ask,
            auto,
            agent,
            voice,
            assist,
            back,
            convo_title,
            strip,
            composer,
            shown_ids: RefCell::new(Vec::new()),
            tab: Cell::new(SlackTab::Direct),
            row_actions: actions.sibling(),
            _actions: actions,
            _links: links,
            mtm,
        };
        page.show_signed_out(None);
        page
    }

    /// The reply box, to put the cursor in when the page shows.
    pub fn input_view(&self) -> Retained<NSView> {
        let field: &NSView = &self.input;
        field.retain()
    }

    /// Puts `text` in the reply box and focuses it.
    pub fn put_draft(&self, text: &str) {
        if let Some(window) = self.view.window() {
            self.set_input(text, &window);
        }
    }

    pub fn take_input(&self) -> String {
        let text = self.input.stringValue().to_string();
        self.input.set_text("");
        text
    }

    /// Puts `text` in the reply box, ready to edit and send.
    /// Whether the reply box holds only whitespace.
    pub fn input_is_empty(&self) -> bool {
        self.input.stringValue().to_string().trim().is_empty()
    }

    /// Nothing is sent: the user reviews it and presses Return or Send.
    pub fn set_input(&self, text: &str, window: &NSWindow) {
        let text = NSString::from_str(text);
        if self.input.currentEditor().is_none() {
            window.makeFirstResponder(Some(&self.input));
        }
        self.input.setStringValue(&text);
        // Caret at the end, ready to edit, rather than the whole draft selected.
        if let Some(editor) = self.input.currentEditor() {
            editor.setString(&text);
            editor.setSelectedRange(NSRange::new(text.length(), 0));
        }
        self.input.fit();
    }

    /// Updates the "write like me" controls above the list.
    pub fn render_voice(&self, state: &VoiceBarState) {
        self.voice.render(state);
    }

    /// Shows the private Claude panel above the reply box.
    pub fn show_assist(&self, content: &AssistView) {
        self.assist.show(content);
        self.layout_transcript();
    }

    pub fn hide_assist(&self) {
        self.assist.hide();
        self.layout_transcript();
    }

    pub fn assist_shown(&self) -> bool {
        self.assist.is_shown()
    }

    /// Fits the transcript between the title and the reply box (and the Claude
    /// panel, when it is shown).
    fn layout_transcript(&self) {
        let size = self.view.bounds().size;
        let mut bottom = widgets::STACKED_COMPOSER_HEIGHT + self.input.extra_height() + MARGIN;
        if self.assist.is_shown() {
            bottom += slack_assist::HEIGHT + MARGIN;
        }
        self.convo_scroll.setFrame(rect(
            MARGIN,
            bottom,
            size.width - 2.0 * MARGIN - bubble::PADDING,
            (size.height - bottom - TOP_HEIGHT).max(0.0),
        ));
    }

    pub fn set_tab(&self, tab: SlackTab) {
        self.tab.set(tab);
    }

    /// The conversation id of row `index`, as last listed.
    pub fn id_at(&self, index: usize) -> Option<String> {
        self.shown_ids.borrow().get(index).cloned()
    }

    /// Whether a conversation (not the list) is currently shown.
    pub fn in_conversation(&self) -> bool {
        !self.convo_scroll.isHidden()
    }

    /// Shows the sign-in prompt, optionally with a status line (e.g. an error).
    pub fn show_signed_out(&self, message: Option<&str>) {
        self.signin.setHidden(false);
        self.status.setHidden(false);
        self.list_scroll.setHidden(true);
        self.voice.view.setHidden(true);
        self.set_conversation_hidden(true);
        let text = message.unwrap_or("Sign in with your usual Slack login.");
        self.status.setAttributedStringValue(&rich_text::attributed(
            text,
            &Attributes::plain(theme::muted(), theme::font(theme::BODY_SIZE)),
        ));
    }

    /// Shows the conversation list.
    pub fn render_list(&self, rows: &[ConversationRow]) {
        self.signin.setHidden(true);
        self.status.setHidden(true);
        self.set_conversation_hidden(true);
        self.list_scroll.setHidden(false);
        self.voice.view.setHidden(false);

        let width = self.list_scroll.contentSize().width;
        for row in self.list.subviews().iter() {
            row.removeFromSuperview();
        }
        self.row_actions.clear();
        let tab = self.tab.get();
        let in_tab = |row: &&ConversationRow| row.direct == (tab == SlackTab::Direct);
        let unread = |direct: bool| {
            rows.iter()
                .filter(|r| r.direct == direct && r.unread)
                .count()
        };
        self.add_tabs(unread(true), unread(false), width);
        let shown: Vec<&ConversationRow> = rows.iter().filter(in_tab).collect();
        let mut ids = Vec::new();
        let mut y = TAB_HEIGHT + CARD_GAP;
        if shown.is_empty() {
            self.add_placeholder(width, y);
            y += ROW_HEIGHT;
        }
        for row in shown {
            self.add_row(ids.len(), row, width, y);
            ids.push(row.id.clone());
            y += CARD_HEIGHT + CARD_GAP;
        }
        let height = y.max(self.list_scroll.contentSize().height);
        self.list.setFrameSize(NSSize::new(width, height));
        self.shown_ids.replace(ids);
    }

    /// Shows one conversation's messages, or one of its threads, and the reply
    /// box; `:name:` found in `emoji` is drawn as that custom emoji, and
    /// attached images from `images`.
    pub fn render_conversation(
        &self,
        in_thread: bool,
        title: &str,
        items: &[TranscriptItem],
        emoji: &HashMap<String, EmojiGlyph>,
        images: &HashMap<String, ImageGlyph>,
    ) {
        self.signin.setHidden(true);
        self.status.setHidden(true);
        self.list_scroll.setHidden(true);
        self.voice.view.setHidden(true);
        self.set_conversation_hidden(false);
        let (back, hint) = match in_thread {
            true => (BACK_TO_CONVERSATION, THREAD_REPLY_HINT),
            false => (BACK_TO_LIST, REPLY_HINT),
        };
        widgets::set_text_button_label(&self.back, back, widgets::ButtonStyle::Link);
        widgets::set_placeholder(&self.input, hint);

        self.convo_title
            .setAttributedStringValue(&rich_text::attributed(
                title,
                &Attributes {
                    heavy: true,
                    ..Attributes::plain(theme::ink(), theme::font(theme::BODY_SIZE))
                },
            ));
        let text = NSMutableAttributedString::new();
        let mut focused = None;
        let mut animated = Vec::new();
        for item in items {
            let piece = match item {
                TranscriptItem::Day(label) => rich_text::slack_day(label),
                TranscriptItem::Run {
                    author,
                    time,
                    mine,
                    bodies,
                    focus,
                } => rich_text::slack_run(author, time, *mine, bodies, *focus),
            };
            // Emoji go in piece by piece, so the focused run's range stays exact.
            let piece = NSMutableAttributedString::initWithAttributedString(
                NSMutableAttributedString::alloc(),
                &piece,
            );
            emoji_text::insert(&piece, emoji, &mut animated);
            emoji_text::insert_images(&piece, images, &mut animated);
            if matches!(item, TranscriptItem::Run { focus: Some(_), .. }) {
                focused = Some(NSRange::new(text.length(), piece.length()));
            }
            text.appendAttributedString(&piece);
        }
        // SAFETY: the text view always owns a text storage.
        if let Some(storage) = unsafe { self.transcript.textStorage() } {
            storage.setAttributedString(&text);
            self.playing.replace(placed(&storage, animated));
            // Lay everything out first: an image's height is only known once
            // laid out, and scrolling before that stops short of the end.
            // SAFETY: a text view built by AppKit owns its layout manager and container.
            let (layout, container) = unsafe {
                (
                    self.transcript.layoutManager(),
                    self.transcript.textContainer(),
                )
            };
            if let (Some(layout), Some(container)) = (layout, container) {
                layout.ensureLayoutForTextContainer(&container);
            }
            self.transcript
                .scrollRangeToVisible(NSRange::new(storage.length(), 0));
            // From the bottom, scrolling back up just enough puts the focused
            // run at the top, with what followed it below.
            if let Some(range) = focused {
                self.transcript.scrollRangeToVisible(range);
            }
        }
    }

    /// Shows the current frame of each animated picture in the transcript;
    /// called on every tick while the conversation is shown.
    pub fn animate(&self) {
        let elapsed = self.clock.elapsed().as_secs_f64();
        // SAFETY: a text view built by AppKit owns its layout manager.
        let Some(layout) = (unsafe { self.transcript.layoutManager() }) else {
            return;
        };
        for playing in self.playing.borrow_mut().iter_mut() {
            let (frame, image) = playing.animated.picture.frame_at(elapsed);
            if frame != playing.frame {
                playing.frame = frame;
                playing.animated.attachment.setImage(Some(image));
                layout.invalidateDisplayForCharacterRange(playing.range);
            }
        }
    }

    fn set_conversation_hidden(&self, hidden: bool) {
        self.convo_scroll.setHidden(hidden);
        self.input.setHidden(hidden);
        self.send.setHidden(hidden);
        self.ask.setHidden(hidden);
        self.auto.setHidden(hidden);
        self.agent.setHidden(hidden);
        if hidden {
            self.assist.hide();
            self.layout_transcript();
        }
        self.back.setHidden(hidden);
        self.convo_title.setHidden(hidden);
        self.strip.setHidden(hidden);
        self.composer.setHidden(hidden);
    }

    /// "Direct · 3" and "Channels · 5": the open tab filled, each with how
    /// many of its conversations have unread messages.
    fn add_tabs(&self, direct_unread: usize, channels_unread: usize, width: f64) {
        let tabs = [
            (SlackTab::Direct, "Direct", direct_unread),
            (SlackTab::Channels, "Channels", channels_unread),
        ];
        let tab_width = TAB_WIDTH.min((width - TAB_GAP) / 2.0);
        for (n, (tab, name, unread)) in tabs.into_iter().enumerate() {
            let label = match unread {
                0 => name.to_string(),
                n => format!("{name} · {n}"),
            };
            let style = if self.tab.get() == tab {
                widgets::ButtonStyle::Primary
            } else {
                widgets::ButtonStyle::Quiet
            };
            let target = self.row_actions.target(SlackAction::Tab(tab));
            let button = widgets::text_button(self.mtm, &label, style, &target, Action::selector());
            button.setToolTip(Some(&NSString::from_str(&format!(
                "{name}: {unread} with unread messages"
            ))));
            button.setFrame(rect(
                n as f64 * (tab_width + TAB_GAP),
                0.0,
                tab_width,
                TAB_HEIGHT,
            ));
            self.list.addSubview(&button);
        }
    }

    fn add_placeholder(&self, width: f64, y: f64) {
        let text = match self.tab.get() {
            SlackTab::Direct => "No direct messages yet.",
            SlackTab::Channels => "No channels yet.",
        };
        let label = NSTextField::labelWithAttributedString(
            &rich_text::attributed(
                text,
                &Attributes::plain(theme::muted(), theme::font(theme::BODY_SIZE)),
            ),
            self.mtm,
        );
        label.setFrame(rect(INDENT, y + 4.0, width - 2.0 * INDENT, ROW_HEIGHT));
        self.list.addSubview(&label);
    }

    /// A card holding the conversation name (clickable), a green square when
    /// it has unread messages, and its mention count and last activity on the right.
    fn add_row(&self, index: usize, row: &ConversationRow, width: f64, y: f64) {
        let card = FlippedView::new(self.mtm, rect(0.0, y, width, CARD_HEIGHT));
        card.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        widgets::paint(&card, Some(&theme::card()), None);
        self.list.addSubview(&card);

        let button = LinkButton::new(
            self.mtm,
            rect(
                CARD_PADDING,
                0.0,
                width - 2.0 * CARD_PADDING - META_WIDTH,
                CARD_HEIGHT,
            ),
            row_title(&row.title, row.unread, false),
            row_title(&row.title, row.unread, true),
            &self.row_actions.target(SlackAction::Open(index)),
            Action::selector(),
        );
        button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        card.addSubview(&button);
        self.add_meta(&card, row, width);
    }

    /// The right-aligned "@2  14:32" of a row, inside its `card`.
    fn add_meta(&self, card: &NSView, row: &ConversationRow, width: f64) {
        let meta = NSMutableAttributedString::new();
        if row.mentions > 0 {
            meta.appendAttributedString(&rich_text::attributed(
                &format!("@{}  ", row.mentions),
                &Attributes {
                    heavy: true,
                    ..Attributes::plain(theme::accent(), theme::font(theme::SMALL_SIZE))
                },
            ));
        }
        meta.appendAttributedString(&rich_text::attributed(
            &row.time,
            &Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE)),
        ));
        let label = NSTextField::labelWithAttributedString(&meta, self.mtm);
        label.setAlignment(NSTextAlignment::Right);
        let height = label.fittingSize().height;
        label.setFrame(rect(
            width - CARD_PADDING - META_WIDTH,
            (CARD_HEIGHT - height) / 2.0,
            META_WIDTH,
            height,
        ));
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        card.addSubview(&label);
    }
}

/// Each of `animated` with where its attachment sits in `text`.
fn placed(text: &NSAttributedString, animated: Vec<Animated>) -> Vec<Playing> {
    if animated.is_empty() {
        return Vec::new();
    }
    let string = text.string().to_string();
    let mut found = Vec::new();
    // Attachments stand on U+FFFC; positions are UTF-16 offsets.
    for (location, unit) in string.encode_utf16().enumerate() {
        if unit != 0xFFFC {
            continue;
        }
        // SAFETY: `location` is inside the text; the attachment key holds an NSTextAttachment.
        let attachment = unsafe {
            text.attribute_atIndex_effectiveRange(
                NSAttachmentAttributeName,
                location,
                std::ptr::null_mut(),
            )
        }
        .and_then(|value| value.downcast::<NSTextAttachment>().ok());
        if let Some(attachment) = attachment {
            found.push((attachment, NSRange::new(location, 1)));
        }
    }
    animated
        .into_iter()
        .filter_map(|animated| {
            let (_, range) = found.iter().find(|(attachment, _)| {
                Retained::as_ptr(attachment) == Retained::as_ptr(&animated.attachment)
            })?;
            Some(Playing {
                range: *range,
                animated,
                frame: 0,
            })
        })
        .collect()
}

/// A conversation name after a green square when unread; bold for unread,
/// accent-underlined on hover.
fn row_title(name: &str, unread: bool, hover: bool) -> Retained<NSAttributedString> {
    let mark = rich_text::attributed(
        if unread { "■  " } else { "   " },
        &Attributes::plain(theme::live(), theme::font(theme::BODY_SIZE)),
    );
    let text = NSMutableAttributedString::new();
    text.appendAttributedString(&mark);
    text.appendAttributedString(&row_name(name, unread, hover));
    text.into_super()
}

fn row_name(text: &str, unread: bool, hover: bool) -> Retained<NSAttributedString> {
    let color = if hover {
        theme::accent()
    } else if unread {
        theme::ink()
    } else {
        theme::muted()
    };
    rich_text::attributed(
        text,
        &Attributes {
            heavy: unread && !hover,
            underline: hover,
            ..Attributes::plain(color, theme::font(theme::BODY_SIZE))
        },
    )
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
