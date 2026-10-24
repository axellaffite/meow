//! Slack on the app side: the live client, the conversation state, and the
//! handlers that tie the Slack page to it.
//!
//! The background [`SlackClient`] streams [`Event`]s that the tick drains in
//! [`App::poll_slack`], the same way [`App::poll_claude`] drains Claude's.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use meow::activity::{Kind, Link, Note};
use meow::slack::assist::Line;
use meow::slack::auth::Credentials;
use meow::slack::cache::{self, SlackCache};
use meow::slack::client::{Command, Event, SlackClient};
use meow::slack::format::{self, NameMap};
use meow::slack::keychain;
use meow::slack::model::{Channel, ChannelKind, Forwarded, Message, SlackImage};
use meow::slack::timeline::{self, Author, Entry};
use meow::source::Source;

use crate::ui::emoji_text;
use crate::ui::rich_text;
use crate::ui::sidebar::Page;
use crate::ui::signin::SignIn;
use crate::ui::slack_page::{ConversationRow, SlackTab, TranscriptItem};

use super::{App, now_secs};

/// The cache is written at most this often while Slack news keeps coming.
const CACHE_SAVE_INTERVAL: Duration = Duration::from_secs(10);

/// The open conversation, readied for a question to Claude.
pub(super) struct AssistContext {
    pub channel: String,
    pub title: String,
    /// The user's own name, as Claude should address them.
    pub me: String,
    pub lines: Vec<Line>,
}

/// Everything the app knows about Slack.
#[derive(Default)]
pub struct SlackState {
    /// The background connection; `None` while signed out.
    client: Option<SlackClient>,
    /// The session in use, for attached images (they sit behind it).
    file_auth: Option<super::downloads::SlackAuth>,
    /// Whether the session in use is the one saved in the Keychain (or just
    /// signed in), so an expired one may be forgotten there. One from the
    /// `MEOW_SLACK_*` variables never touches the Keychain.
    saved_session: bool,
    /// Whether `auth.test` has confirmed the session.
    signed_in: bool,
    /// Our own user id, to tell our messages apart.
    me: Option<String>,
    /// The conversations, most recent first.
    channels: Vec<Channel>,
    /// Names learned for users (and channels), for titles and `mrkdwn`.
    names: NameMap,
    /// The open conversation and its messages.
    open: Option<String>,
    messages: Vec<Message>,
    /// The thread open in that conversation (its parent's `ts`), and its
    /// messages: the parent, then the replies.
    thread: Option<String>,
    thread_messages: Vec<Message>,
    /// The message to scroll to and mark in the open conversation, as `ts`
    /// values, best first (the message, then its thread); empty for the latest.
    focus: Vec<String>,
    /// A line to show while signed out (a prompt or an error).
    status: Option<String>,
    /// The sign-in window, while the user is signing in.
    signin: Option<SignIn>,
    /// What was last seen of Slack, shown at once on launch.
    cache: SlackCache,
    cache_path: Option<PathBuf>,
    /// Changes not yet written, and when it was last written.
    cache_dirty: bool,
    cache_saved_at: Option<Instant>,
}

impl SlackState {
    /// Loads stored or configured credentials and connects if any are found.
    pub fn new() -> Self {
        let cache_path = cache::default_path();
        let cache = cache_path
            .as_deref()
            .map(SlackCache::load)
            .transpose()
            .unwrap_or_else(|error| {
                // Only a cache: start empty, and overwrite it on the next save.
                eprintln!("meow: {error}");
                None
            })
            .unwrap_or_default();
        let mut state = Self {
            cache,
            cache_path,
            ..Self::default()
        };
        // Snapshots show sample messages; they never sign in.
        let snapshot = std::env::var_os("MEOW_SNAPSHOT_DIR").is_some();
        if let Some((creds, saved)) = load_credentials().filter(|_| !snapshot) {
            state.saved_session = saved;
            state.connect(creds);
        }
        state
    }

    /// Connects, showing what the cache holds for this account meanwhile.
    fn connect(&mut self, creds: Credentials) {
        self.file_auth = Some(super::downloads::SlackAuth {
            token: creds.token.clone(),
            cookie: creds.cookie.clone(),
        });
        self.adopt_cache(&creds.user_id);
        self.channels = self.cache.channels.clone();
        self.names.users.extend(self.cache.names());
        let known = self.cache.fresh_users(now_secs());
        self.client = Some(SlackClient::connect(creds, known));
        self.status = Some("Connecting to Slack…".to_string());
    }

    /// Keeps the cache only if it belongs to `owner`; another account's
    /// conversations are dropped from view too.
    fn adopt_cache(&mut self, owner: &str) {
        let before = self.cache.owner.clone();
        self.cache = std::mem::take(&mut self.cache).for_owner(owner);
        if before != self.cache.owner && !before.is_empty() {
            self.channels.clear();
            self.messages.clear();
            self.open = None;
            self.close_thread();
            self.names = NameMap::default();
        }
        self.cache_dirty |= before != self.cache.owner;
    }

    /// Connecting, with cached conversations to show until Slack answers.
    fn showing_cached(&self) -> bool {
        !self.signed_in && self.client.is_some() && !self.channels.is_empty()
    }

    /// Writes the cache if it changed, unless it was written moments ago.
    fn save_cache_soon(&mut self) {
        let recent = self
            .cache_saved_at
            .is_some_and(|at| at.elapsed() < CACHE_SAVE_INTERVAL);
        if !recent {
            self.save_cache();
        }
    }

    /// Writes the cache now if it changed.
    pub(super) fn save_cache(&mut self) {
        let Some(path) = &self.cache_path else { return };
        if !self.cache_dirty {
            return;
        }
        if let Err(error) = self.cache.save(path) {
            eprintln!("meow: {error}");
        }
        self.cache_dirty = false;
        self.cache_saved_at = Some(Instant::now());
    }

    /// The conversation rows for the list, titled and sorted most-recent first.
    fn rows(&self, now: u64) -> Vec<ConversationRow> {
        self.channels
            .iter()
            .map(|channel| ConversationRow {
                id: channel.id.clone(),
                title: channel_title(channel, &self.names),
                unread: channel.unread,
                mentions: channel.mentions,
                direct: channel.kind != ChannelKind::Channel,
                time: channel
                    .latest_ts
                    .as_deref()
                    .map(|ts| timeline::list_label(ts, now))
                    .unwrap_or_default(),
            })
            .collect()
    }

    /// The messages on screen: the open thread's, else the conversation's.
    fn shown(&self) -> &[Message] {
        match self.thread {
            Some(_) => &self.thread_messages,
            None => &self.messages,
        }
    }

    /// Leaves the open thread, if any, for its conversation.
    fn close_thread(&mut self) {
        self.thread = None;
        self.thread_messages.clear();
    }

    /// The open conversation or thread, grouped into days and author runs,
    /// with names resolved and bodies in Markdown.
    fn rendered(&self, now: u64) -> Vec<TranscriptItem> {
        let shown = self.shown();
        let focus = timeline::first_loaded(shown, &self.focus);
        let entries = match self.thread {
            Some(_) => timeline::thread(shown, now),
            None => timeline::timeline(shown, now),
        };
        let place = |message: &Message| match self.thread {
            Some(_) => Shown::Thread,
            None => Shown::Channel(
                shown
                    .iter()
                    .position(|m| m.ts == message.ts)
                    .unwrap_or_default(),
            ),
        };
        entries
            .into_iter()
            .map(|entry| match entry {
                Entry::Day(label) => TranscriptItem::Day(label),
                Entry::Run {
                    author,
                    time,
                    messages,
                } => TranscriptItem::Run {
                    author: match author {
                        Author::User(id) => self.name_of(Some(id)),
                        Author::Bot(name) => name.to_string(),
                        Author::Unknown => self.name_of(None),
                    },
                    time,
                    mine: matches!(author, Author::User(id) if Some(id) == self.me.as_deref()),
                    focus: messages
                        .iter()
                        .position(|message| Some(message.ts.as_str()) == focus),
                    bodies: messages
                        .into_iter()
                        .map(|message| message_body(message, &self.names, place(message)))
                        .collect(),
                },
            })
            .collect()
    }

    /// The title of the open conversation, or of its open thread.
    fn open_title(&self) -> String {
        let title = self
            .open
            .as_ref()
            .and_then(|id| self.channels.iter().find(|c| &c.id == id))
            .map(|channel| channel_title(channel, &self.names))
            .unwrap_or_else(|| "Slack".to_string());
        match self.thread {
            Some(_) => format!("Thread in {title}"),
            None => title,
        }
    }

    /// Hands `command` to the worker; `false` when not connected.
    pub(super) fn request(&self, command: Command) -> bool {
        match &self.client {
            Some(client) if self.signed_in => {
                client.send(command);
                true
            }
            _ => false,
        }
    }

    /// The user's own Slack id, once connected.
    pub(super) fn my_id(&self) -> Option<&str> {
        self.me.as_deref()
    }

    /// The user's own name, as Claude should call them.
    pub(super) fn my_name(&self) -> String {
        self.name_of(self.me.as_deref())
    }

    /// What Claude needs to answer a question about the open conversation.
    pub(super) fn assist_context(&self) -> Option<AssistContext> {
        let channel = self.open.clone()?;
        let lines = self
            .shown()
            .iter()
            .map(|message| Line {
                ts: message.ts.clone(),
                author: match (&message.user, &message.bot_name) {
                    (None, Some(bot)) => bot.clone(),
                    (user, _) => self.name_of(user.as_deref()),
                },
                text: message_body(message, &self.names, Shown::Text),
            })
            .collect();
        Some(AssistContext {
            channel,
            title: self.open_title(),
            me: self.my_name(),
            lines,
        })
    }

    /// The session, to download attached images.
    pub(super) fn file_auth(&self) -> Option<super::downloads::SlackAuth> {
        self.file_auth.clone()
    }

    /// The open conversation's id, if one is open.
    pub(super) fn open_channel(&self) -> Option<&str> {
        self.open.as_deref()
    }

    /// The saved messages of `channel`, oldest first.
    pub(super) fn history(&self, channel: &str) -> &[Message] {
        self.cache.history(channel)
    }

    /// `message` as text, for Claude.
    pub(super) fn text_of(&self, message: &Message) -> String {
        message_body(message, &self.names, Shown::Text)
    }

    pub(super) fn name_of(&self, id: Option<&str>) -> String {
        id.and_then(|id| self.names.users.get(id).cloned())
            .or_else(|| id.map(str::to_string))
            .unwrap_or_else(|| "someone".to_string())
    }
}

impl App {
    /// Opens the Slack page, connecting state to the view.
    pub(super) fn show_slack(&mut self) {
        if !self.chat.is_visible() {
            self.open_window();
        }
        self.render_slack();
        self.chat.show_page(Page::Slack);
    }

    pub(super) fn sign_in_slack(&mut self) {
        // Configured credentials win; otherwise open the embedded web sign-in.
        match load_credentials() {
            Some((creds, saved)) => {
                self.slack.saved_session = saved;
                self.slack.connect(creds);
            }
            None => {
                if self.slack.signin.is_none() {
                    self.slack.signin = Some(SignIn::present(self.mtm));
                }
                self.slack.status = Some("Finish signing in in the Slack window…".to_string());
            }
        }
        self.render_slack();
    }

    pub(super) fn open_slack_conversation(&mut self, row: usize) {
        if let Some(id) = self.chat.slack().id_at(row) {
            self.open_slack_channel(id);
        }
    }

    /// Opens conversation `id` on the Slack page, on its latest messages.
    pub(super) fn open_slack_channel(&mut self, id: String) {
        self.open_slack_at(id, Vec::new());
    }

    /// Opens conversation `id` scrolled to message `ts` and marked. A reply in
    /// a thread opens that thread instead, over the conversation scrolled to
    /// the parent.
    pub(super) fn open_slack_message(&mut self, id: String, ts: String, thread_ts: Option<String>) {
        match thread_ts.filter(|parent| *parent != ts) {
            Some(parent) => {
                self.open_slack_at(id, vec![parent.clone()]);
                self.show_slack_thread(parent, vec![ts]);
            }
            None => self.open_slack_at(id, vec![ts]),
        }
    }

    /// Opens the thread of the open conversation's message at `index` (as
    /// last rendered), when it starts one.
    pub(super) fn open_slack_thread(&mut self, index: usize) {
        let parent = self
            .slack
            .messages
            .get(index)
            .filter(|message| message.reply_count > 0)
            .map(|message| message.thread_ts.clone().unwrap_or(message.ts.clone()));
        if let Some(parent) = parent {
            self.show_slack_thread(parent, Vec::new());
        }
    }

    /// Shows thread `parent` of the open conversation, scrolled to `focus`
    /// (as `ts` values, best first) or to its latest reply.
    fn show_slack_thread(&mut self, parent: String, focus: Vec<String>) {
        let Some(channel) = self.slack.open.clone() else {
            return;
        };
        self.close_slack_assist();
        // The parent at once, until Slack sends the replies.
        self.slack.thread_messages = self
            .slack
            .messages
            .iter()
            .filter(|message| message.ts == parent)
            .cloned()
            .collect();
        self.slack.thread = Some(parent.clone());
        self.slack.focus = focus;
        self.render_slack();
        if let Some(client) = &self.slack.client {
            client.send(Command::OpenThread {
                channel,
                thread_ts: parent,
            });
        }
    }

    fn open_slack_at(&mut self, id: String, focus: Vec<String>) {
        self.close_slack_assist();
        self.slack.open = Some(id.clone());
        self.slack.close_thread();
        self.slack.focus = focus;
        self.read_slack_activity(&id);
        // What was last seen, until Slack sends the latest.
        self.slack.messages = self.slack.cache.history(&id).to_vec();
        // Opening marks it read (the worker tells Slack); reflect that at once.
        if let Some(channel) = self.slack.channels.iter_mut().find(|c| c.id == id) {
            channel.unread = false;
            channel.mentions = 0;
        }
        self.render_slack();
        self.show_slack_agent(&id);
        if let Some(client) = &self.slack.client {
            client.send(Command::Open(id));
        }
    }

    /// Switches the conversation list to `tab`.
    pub(super) fn show_slack_tab(&mut self, tab: SlackTab) {
        self.chat.slack().set_tab(tab);
        self.render_slack();
    }

    /// Back from a thread to its conversation, or from a conversation to the list.
    pub(super) fn slack_back(&mut self) {
        self.close_slack_assist();
        if self.slack.thread.is_some() {
            self.slack.close_thread();
            self.slack.focus.clear();
            if let Some(client) = &self.slack.client {
                client.send(Command::CloseThread);
            }
            self.render_slack();
            return;
        }
        self.slack.open = None;
        self.slack.focus.clear();
        self.render_slack();
        self.chat.show_page(Page::Slack);
    }

    pub(super) fn send_slack(&mut self) {
        let text = self.chat.slack().take_input();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        // Back to the latest, where the reply will show.
        self.slack.focus.clear();
        if let (Some(channel), Some(client)) = (self.slack.open.clone(), self.slack.client.as_ref())
        {
            client.send(Command::Send {
                channel,
                text: text.to_string(),
                thread_ts: self.slack.thread.clone(),
            });
        }
    }

    /// Drains the sign-in flow and everything the worker has sent since last tick.
    pub(super) fn poll_slack(&mut self) {
        self.poll_sign_in();
        let events: Vec<Event> = match &self.slack.client {
            Some(client) => client.events.try_iter().collect(),
            None => return,
        };
        for event in events {
            self.on_slack_event(event);
        }
        self.slack.save_cache_soon();
    }

    /// Advances an in-progress web sign-in; connects once it yields credentials.
    fn poll_sign_in(&mut self) {
        if self.slack.signin.is_none() {
            return;
        }
        let open = self.slack.signin.as_ref().is_some_and(SignIn::is_open);
        if !open {
            // The user closed the window without finishing.
            self.slack.signin = None;
            if !self.slack.signed_in {
                self.slack.status = Some("Sign in with your usual Slack login.".to_string());
                if self.slack_visible() {
                    self.render_slack();
                }
            }
            return;
        }
        let credentials = self.slack.signin.as_mut().and_then(SignIn::poll);
        if let Some(credentials) = credentials {
            if let Some(signin) = self.slack.signin.take() {
                signin.close();
            }
            if let Err(error) = keychain::save(&credentials) {
                eprintln!("meow: {error}");
            }
            self.slack.saved_session = true;
            self.slack.connect(credentials);
            if self.slack_visible() {
                self.render_slack();
            }
        }
    }

    fn on_slack_event(&mut self, event: Event) {
        match event {
            Event::Connected(auth) => {
                self.slack.signed_in = true;
                // First, as another account's cache clears the names too.
                self.slack.adopt_cache(&auth.user_id);
                // Until users.info answers, our own name is the one auth.test gives.
                self.slack
                    .names
                    .users
                    .entry(auth.user_id.clone())
                    .or_insert(auth.user);
                self.slack.me = Some(auth.user_id);
                self.slack.status = None;
            }
            Event::Conversations(mut channels) => {
                channels.sort_by(|a, b| b.latest_ts.cmp(&a.latest_ts));
                self.slack.cache.set_channels(channels.clone());
                self.slack.cache_dirty = true;
                self.slack.channels = channels;
            }
            Event::Users(users) => {
                self.slack.cache.add_users(&users, now_secs());
                self.slack.cache_dirty = true;
                for user in users {
                    self.slack
                        .names
                        .users
                        .insert(user.id.clone(), user.name().to_string());
                }
            }
            Event::History { channel, messages } => {
                self.slack.cache.set_history(&channel, &messages);
                self.slack.cache_dirty = true;
                if self.slack.open.as_deref() == Some(&channel) {
                    self.slack.messages = messages;
                }
            }
            Event::New { channel, messages } => {
                self.slack.cache.append(&channel, &messages);
                self.slack.cache_dirty = true;
                self.notify_slack(&channel, &messages);
                if self.slack.open.as_deref() == Some(&channel) {
                    let from_others = messages
                        .iter()
                        .any(|m| m.user.as_deref() != self.slack.me.as_deref());
                    self.slack.messages.extend(messages);
                    // Something new: show the latest again, unless reading a thread.
                    if self.slack.thread.is_none() {
                        self.slack.focus.clear();
                    }
                    if from_others && !self.slack_visible() {
                        self.unread = true;
                    }
                } else {
                    self.unread = true;
                }
            }
            Event::Thread {
                channel,
                thread_ts,
                messages,
            } => {
                let open = self.slack.open.as_deref() == Some(&channel)
                    && self.slack.thread.as_deref() == Some(&thread_ts);
                if open {
                    // Replies were already shown: this is news, so show the latest.
                    if self.slack.thread_messages.len() > 1 {
                        self.slack.focus.clear();
                    }
                    self.slack.thread_messages = messages;
                }
            }
            Event::Sent { .. } => {}
            Event::Error(message) => {
                if self.slack.signed_in {
                    eprintln!("meow: slack: {message}");
                } else {
                    self.slack.status = Some(message);
                }
            }
            Event::MyMessages(result) => self.on_my_messages(result),
            Event::CustomEmoji(raw) => self.on_custom_emoji(&raw),
            Event::People { channel, people } => self.on_people(channel, people),
            Event::Expired => {
                self.close_slack_assist();
                self.slack.signed_in = false;
                self.slack.client = None;
                self.slack.open = None;
                self.slack.close_thread();
                if self.slack.saved_session {
                    let _ = keychain::clear();
                }
                self.slack.status = Some("Your Slack session ended. Sign in again.".to_string());
            }
        }
        if self.slack_visible() {
            self.render_slack();
        }
    }

    /// Draws the Slack page for the current state: signed out, a list, or a chat.
    pub(super) fn render_slack(&mut self) {
        if !self.slack.signed_in && !self.slack.showing_cached() {
            self.chat
                .slack()
                .show_signed_out(self.slack.status.as_deref());
            return;
        }
        if self.slack.open.is_some() {
            let messages = self.slack.shown().to_vec();
            self.want_images(&messages);
        }
        let page = self.chat.slack();
        match &self.slack.open {
            Some(_) => page.render_conversation(
                self.slack.thread.is_some(),
                &self.slack.open_title(),
                &self.slack.rendered(now_secs()),
                &self.emoji.glyphs,
                &self.emoji.images,
            ),
            None => {
                page.render_list(&self.slack.rows(now_secs()));
                page.render_voice(&self.voice_bar_state());
            }
        }
    }

    /// A DM or a mention from someone else, out of sight, goes to Activity.
    fn notify_slack(&mut self, channel: &str, messages: &[Message]) {
        let watching = self.slack_visible() && self.slack.open.as_deref() == Some(channel);
        let Some(conversation) = self.slack.channels.iter().find(|c| c.id == channel) else {
            return;
        };
        let me = self.slack.me.as_deref();
        let for_me: Vec<&Message> = messages
            .iter()
            .filter(|m| m.user.is_some() && m.user.as_deref() != me)
            .filter(|m| is_for_me(conversation.kind, &m.text, me))
            .collect();
        let Some(last) = for_me.last() else { return };
        let sender = self.slack.name_of(last.user.as_deref());
        let place = channel_title(conversation, &self.slack.names);
        let mut title = if conversation.kind == ChannelKind::Im {
            sender
        } else {
            format!("{sender} in {place}")
        };
        // Even when reading it: preparing the work is what saves time.
        let last = (*last).clone();
        self.consider_for_autopilot(channel, &last, &title);
        if watching {
            return;
        }
        if for_me.len() > 1 {
            title.push_str(&format!(" ({} messages)", for_me.len()));
        }
        let detail = message_body(&last, &self.slack.names, Shown::Text);
        // One item per sender and conversation: their last message replaces the earlier ones.
        let key = format!(
            "slack:{channel}:{}",
            last.user.as_deref().unwrap_or_default()
        );
        self.notify(Note {
            kind: Kind::Slack,
            title,
            detail: Some(detail),
            link: Some(Link::Slack {
                channel: channel.to_string(),
                ts: Some(last.ts.clone()),
                thread_ts: last.thread_ts.clone(),
            }),
            sources: vec![Source::Slack],
            key: Some(key),
        });
    }

    /// Whether a Slack conversation (not the list) is on screen.
    pub(super) fn slack_conversation_shown(&self) -> bool {
        self.slack_visible() && self.slack.open.is_some()
    }

    fn slack_visible(&self) -> bool {
        self.chat.is_visible() && self.chat.page() == Page::Slack
    }
}

/// Credentials from the MEOW_SLACK_* variables, else the Keychain; `true`
/// when they come from the Keychain.
fn load_credentials() -> Option<(Credentials, bool)> {
    env_credentials()
        .map(|creds| (creds, false))
        .or_else(|| keychain::load().ok().flatten().map(|creds| (creds, true)))
}

fn env_credentials() -> Option<Credentials> {
    let token = std::env::var("MEOW_SLACK_TOKEN").ok()?;
    let cookie = std::env::var("MEOW_SLACK_COOKIE").ok()?;
    let domain = std::env::var("MEOW_SLACK_DOMAIN").ok()?;
    Some(Credentials {
        token,
        cookie,
        domain,
        team_id: String::new(),
        user_id: String::new(),
    })
}

/// Where a message body is shown, which decides how its images and replies read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// In the conversation, at this index of its messages: images drawn, and
    /// the reply count a link opening the thread.
    Channel(usize),
    /// In its thread: images drawn, the replies being right below.
    Thread,
    /// As text, for Claude or a notification: images named.
    Text,
}

/// A message's text in Markdown, with what Slack shows besides text: attached
/// images and files, forwarded messages, a link or bot card summary, and a
/// thread's reply count. In the transcript each image is a marker swapped for
/// the picture; elsewhere it is named.
fn message_body(message: &Message, names: &NameMap, shown: Shown) -> String {
    let text = format::to_markdown(&message.text, names);
    let mut parts = Vec::new();
    if !text.trim().is_empty() {
        parts.push(text);
    } else if let Some(summary) = &message.attachment {
        parts.push(format!("_{}_", summary.trim()));
    }
    parts.extend(attached(&message.images, &message.files, shown));
    // A blank line closes each quote, or what follows would join it.
    parts.extend(
        message
            .forwarded
            .iter()
            .map(|forwarded| format!("{}\n", forwarded_quote(forwarded, names, shown))),
    );
    let replies = match message.reply_count {
        0 => None,
        n => Some(format!("↳ {}", timeline::replies_label(n as usize))),
    };
    match (replies, shown) {
        (Some(replies), Shown::Channel(index)) => {
            parts.push(format!("[{replies}]({})", rich_text::thread_link(index)))
        }
        (Some(replies), Shown::Text) => parts.push(replies),
        (Some(_), Shown::Thread) | (None, _) => {}
    }
    if parts.is_empty() {
        parts.push("_(no text)_".to_string());
    }
    parts.join("\n").trim_end().to_string()
}

/// One line per attached image and file.
fn attached(images: &[SlackImage], files: &[String], shown: Shown) -> Vec<String> {
    images
        .iter()
        .map(|image| match shown {
            Shown::Channel(_) | Shown::Thread => emoji_text::image_marker(&image.id),
            Shown::Text => format!("🖼 {}", image.title),
        })
        .chain(files.iter().map(|name| format!("📎 {name}")))
        .collect()
}

/// A forwarded message as a Markdown quote: who wrote it and where, then what
/// it says and carries. In the transcript it links to the original.
fn forwarded_quote(forwarded: &Forwarded, names: &NameMap, shown: Shown) -> String {
    let author = forwarded
        .user
        .as_ref()
        .and_then(|id| names.users.get(id))
        .map(String::as_str)
        .or(Some(forwarded.author.trim()).filter(|name| !name.is_empty()))
        .unwrap_or("Someone");
    let place = forwarded
        .channel
        .as_ref()
        .map(|channel| format!(" in #{channel}"))
        .unwrap_or_default();
    let link = match (&forwarded.link, shown) {
        (Some(url), Shown::Channel(_) | Shown::Thread) => format!(" · [Open in Slack]({url})"),
        _ => String::new(),
    };
    let text = format::to_markdown(&forwarded.text, names);
    std::iter::once(format!("↪ **{author}**{place}{link}"))
        .chain(text.trim().lines().map(str::to_string))
        .chain(attached(&forwarded.images, &forwarded.files, shown))
        .map(|line| match line.trim_end() {
            "" => ">".to_string(),
            line => format!("> {line}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a message is for the user: any message in a direct or group
/// message, or one that mentions them in a channel.
fn is_for_me(kind: ChannelKind, text: &str, me: Option<&str>) -> bool {
    match kind {
        ChannelKind::Im | ChannelKind::Mpim => true,
        ChannelKind::Channel => me.is_some_and(|me| text.contains(&format!("<@{me}>"))),
    }
}

/// A readable title for a conversation.
fn channel_title(channel: &Channel, names: &NameMap) -> String {
    match channel.kind {
        ChannelKind::Im => channel
            .user
            .as_ref()
            .map(|id| names.users.get(id).cloned().unwrap_or_else(|| id.clone()))
            .unwrap_or_else(|| "Direct message".to_string()),
        ChannelKind::Mpim => prettify_mpim(&channel.name),
        ChannelKind::Channel => format!("#{}", channel.name),
    }
}

/// Turns Slack's `mpdm-alice--bob--carol-1` group name into `alice, bob, carol`.
fn prettify_mpim(raw: &str) -> String {
    let trimmed = raw.strip_prefix("mpdm-").unwrap_or(raw);
    let trimmed = trimmed
        .rsplit_once('-')
        .filter(|(_, tail)| tail.chars().all(|c| c.is_ascii_digit()))
        .map(|(head, _)| head)
        .unwrap_or(trimmed);
    let names: Vec<&str> = trimmed.split("--").filter(|s| !s.is_empty()).collect();
    if names.is_empty() {
        "Group message".to_string()
    } else {
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_messages_and_mentions_are_for_me() {
        assert!(is_for_me(ChannelKind::Im, "hi", Some("U1")));
        assert!(is_for_me(ChannelKind::Mpim, "hi all", None));
        assert!(is_for_me(ChannelKind::Channel, "ping <@U1>", Some("U1")));
        assert!(!is_for_me(ChannelKind::Channel, "ping <@U2>", Some("U1")));
        assert!(!is_for_me(ChannelKind::Channel, "<@U1>", None));
    }

    #[test]
    fn mpim_name_is_prettified() {
        assert_eq!(
            prettify_mpim("mpdm-alice--bob--carol-1"),
            "alice, bob, carol"
        );
        assert_eq!(prettify_mpim("mpdm-alice--bob-1"), "alice, bob");
    }

    #[test]
    fn odd_mpim_names_degrade_gracefully() {
        assert_eq!(prettify_mpim(""), "Group message");
        assert_eq!(prettify_mpim("weird"), "weird");
    }

    fn message(text: &str) -> Message {
        Message {
            ts: "1.0".into(),
            user: Some("U1".into()),
            text: text.into(),
            thread_ts: None,
            reply_count: 0,
            files: Vec::new(),
            images: Vec::new(),
            attachment: None,
            bot_name: None,
            forwarded: Vec::new(),
        }
    }

    #[test]
    fn body_shows_files_and_replies_alongside_text() {
        let names = NameMap::default();
        let msg = Message {
            files: vec!["plan.pdf".into()],
            reply_count: 3,
            ..message("see attached")
        };
        assert_eq!(
            message_body(&msg, &names, Shown::Text),
            "see attached\n📎 plan.pdf\n↳ 3 replies"
        );
    }

    #[test]
    fn replies_open_the_thread_from_the_conversation_only() {
        let names = NameMap::default();
        let parent = Message {
            thread_ts: Some("1.0".into()),
            reply_count: 1,
            ..message("plan?")
        };
        assert_eq!(
            message_body(&parent, &names, Shown::Channel(4)),
            format!("plan?\n[↳ 1 reply]({})", rich_text::thread_link(4))
        );
        assert_eq!(message_body(&parent, &names, Shown::Thread), "plan?");
    }

    #[test]
    fn images_are_markers_in_the_transcript_and_names_elsewhere() {
        let names = NameMap::default();
        let msg = Message {
            images: vec![meow::slack::model::SlackImage {
                id: "F1".into(),
                title: "Screenshot".into(),
                thumb: "https://files.slack.com/F1/480.png".into(),
                permalink: None,
            }],
            ..message("look")
        };
        let marked = format!("look\n{}", emoji_text::image_marker("F1"));
        assert_eq!(message_body(&msg, &names, Shown::Channel(0)), marked);
        assert_eq!(message_body(&msg, &names, Shown::Thread), marked);
        assert_eq!(
            message_body(&msg, &names, Shown::Text),
            "look\n🖼 Screenshot"
        );
    }

    fn forward() -> Forwarded {
        Forwarded {
            user: Some("U2".into()),
            author: "Bob B".into(),
            channel: Some("general".into()),
            text: "ship it\n\ntoday".into(),
            files: vec!["notes.pdf".into()],
            images: vec![meow::slack::model::SlackImage {
                id: "F1".into(),
                title: "Screenshot".into(),
                thumb: "https://files.slack.com/F1/480.png".into(),
                permalink: None,
            }],
            link: Some("https://x.slack.com/archives/C1/p1000".into()),
        }
    }

    #[test]
    fn forwarded_messages_are_quoted_with_author_and_place() {
        let mut names = NameMap::default();
        names.users.insert("U2".to_string(), "bob".to_string());
        let msg = Message {
            forwarded: vec![forward()],
            reply_count: 1,
            ..message("fyi")
        };
        assert_eq!(
            message_body(&msg, &names, Shown::Text),
            "fyi\n> ↪ **bob** in #general\n> ship it\n>\n> today\n\
             > 🖼 Screenshot\n> 📎 notes.pdf\n\n↳ 1 reply"
        );
    }

    #[test]
    fn forwarded_messages_link_to_the_original_in_the_transcript() {
        let names = NameMap::default();
        let msg = Message {
            forwarded: vec![Forwarded {
                text: String::new(),
                files: Vec::new(),
                ..forward()
            }],
            ..message("")
        };
        assert_eq!(
            message_body(&msg, &names, Shown::Thread),
            format!(
                "> ↪ **Bob B** in #general · [Open in Slack](https://x.slack.com/archives/C1/p1000)\n> {}",
                emoji_text::image_marker("F1")
            )
        );
    }

    #[test]
    fn forwards_without_a_known_author_or_channel_still_read() {
        let names = NameMap::default();
        let bare = Forwarded {
            user: None,
            author: " ".into(),
            channel: None,
            text: "psst".into(),
            files: Vec::new(),
            images: Vec::new(),
            link: None,
        };
        let msg = Message {
            forwarded: vec![bare.clone(), bare],
            ..message("")
        };
        assert_eq!(
            message_body(&msg, &names, Shown::Channel(0)),
            "> ↪ **Someone**\n> psst\n\n> ↪ **Someone**\n> psst"
        );
    }

    #[test]
    fn empty_body_falls_back_to_attachment_then_placeholder() {
        let names = NameMap::default();
        let card = Message {
            attachment: Some("Build passed".into()),
            ..message("")
        };
        assert_eq!(message_body(&card, &names, Shown::Text), "_Build passed_");
        assert_eq!(
            message_body(&message(""), &names, Shown::Text),
            "_(no text)_"
        );
    }

    #[test]
    fn im_title_uses_resolved_name_then_id() {
        let mut names = NameMap::default();
        names.users.insert("U2".to_string(), "bob".to_string());
        let channel = Channel {
            id: "D1".into(),
            kind: ChannelKind::Im,
            name: String::new(),
            user: Some("U2".into()),
            latest_ts: None,
            unread: false,
            mentions: 0,
        };
        assert_eq!(channel_title(&channel, &names), "bob");

        let unknown = Channel {
            user: Some("U9".into()),
            ..channel
        };
        assert_eq!(channel_title(&unknown, &names), "U9");
    }
}
