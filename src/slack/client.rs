//! The live Slack client: a background worker that owns the Web API and reaches
//! the 30 Hz tick over channels, like [`crate::claude::Run`].
//!
//! Slack's live websocket ([`super::socket`]) says what changed as it happens,
//! and the worker fetches just that. Polling stays as the fallback: every few
//! seconds while the websocket is down, and rarely once it is up, to catch up
//! on anything missed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::api::{ApiError, HISTORY_LIMIT, SlackApi};
use super::auth::Credentials;
use super::http::{Http, Web};
use super::model::{Activity, AuthTest, Channel, ChannelKind, Message, User};
use super::socket::{self, Change, Posted};

/// How often an idle worker re-checks the open conversation without the websocket.
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// How often it still does while the websocket is up, in case a change slipped by.
const LIVE_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// How often polling re-lists conversations, for changes the websocket does not announce.
const RELIST_EVERY: Duration = Duration::from_secs(30);
/// The first wait before reopening a lost websocket; it doubles up to the max.
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);
/// Up this long, a lost websocket is retried from the first wait again.
const STABLE_AFTER: Duration = Duration::from_secs(30);
/// At most this many people are looked up for an invite.
const MAX_PEOPLE: usize = 100;
/// Conversation types shown: DMs, group DMs and the user's channels.
const TYPES: &str = super::api::ALL_TYPES;

/// A request from the main thread to the worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Reload the list of conversations.
    Refresh,
    /// Load a conversation's recent messages and mark it read.
    Open(String),
    /// Load a thread of a conversation, and keep it fresh while it is open.
    OpenThread { channel: String, thread_ts: String },
    /// Stop keeping the open thread fresh.
    CloseThread,
    /// Send a message, optionally as a thread reply.
    Send {
        channel: String,
        text: String,
        thread_ts: Option<String>,
    },
    /// Look up everyone in a conversation, with their email and time zone.
    People(String),
    /// Fetch the user's own recent messages, to learn how they write.
    FetchMyMessages { limit: usize },
    /// Something changed on Slack. Sent by the websocket, not by the app.
    Live(Change),
    /// Stop the worker.
    Stop,
}

/// Something the worker learned, for the tick to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The credentials work; here is who we are.
    Connected(AuthTest),
    /// The full conversation list.
    Conversations(Vec<Channel>),
    /// Names learned for some users.
    Users(Vec<User>),
    /// The recent messages of the conversation just opened.
    History {
        channel: String,
        messages: Vec<Message>,
    },
    /// Messages that arrived since last seen: in the open conversation, or
    /// for the user in another one (right after its `History`).
    New {
        channel: String,
        messages: Vec<Message>,
    },
    /// New replies for the user in a thread, with the whole thread: its
    /// parent, then the replies oldest-first.
    Replies {
        channel: String,
        thread: Vec<Message>,
        fresh: Vec<Message>,
    },
    /// The whole open thread: its parent, then the replies oldest-first. Sent
    /// when it is opened and again whenever it changes.
    Thread {
        channel: String,
        thread_ts: String,
        messages: Vec<Message>,
    },
    /// A message was sent to this channel.
    Sent { channel: String },
    /// Something went wrong, with a message to show.
    Error(String),
    /// The session is no longer valid; the user must sign in again.
    Expired,
    /// The people in a conversation, or why they could not be looked up.
    People {
        channel: String,
        people: Result<Vec<User>, String>,
    },
    /// The user's own recent messages, newest first, or why they could not be fetched.
    MyMessages(Result<Vec<String>, String>),
    /// The workspace's custom emoji: names to an image URL or `alias:name`.
    CustomEmoji(HashMap<String, String>),
}

/// Owns the worker thread; commands go in, events come out on `events`.
pub struct SlackClient {
    commands: Sender<Command>,
    pub events: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
}

impl SlackClient {
    /// Connects with `creds` and begins polling in the background. `known`
    /// people (from the cache) are not looked up again.
    pub fn connect(creds: Credentials, known: Vec<User>) -> Self {
        let (commands_tx, commands_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        let live_tx = commands_tx.clone();
        let handle = thread::spawn(move || {
            Worker::new(SlackApi::new(Web::new(), creds), events_tx)
                .knowing(known)
                .run(commands_rx, live_tx);
        });
        Self {
            commands: commands_tx,
            events: events_rx,
            handle: Some(handle),
        }
    }

    /// Queues a command for the worker. A dropped worker swallows it harmlessly.
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for SlackClient {
    fn drop(&mut self) {
        // Ask the worker to stop and let it wind down on its own; joining could
        // block the app for the length of an in-flight request.
        let _ = self.commands.send(Command::Stop);
        let _ = self.handle.take();
    }
}

/// Where the websocket stands, and how long to wait before reopening it.
#[derive(Debug)]
enum LiveState {
    /// Down since start or lost; reopen at `at`, `after` waiting that long.
    Down {
        at: Instant,
        after: Duration,
    },
    Connecting,
    Up {
        since: Instant,
    },
}

struct Live {
    state: LiveState,
    /// The wait before the next reopen, should this connection be lost.
    backoff: Duration,
}

impl Live {
    fn new() -> Self {
        Self {
            state: LiveState::Down {
                at: Instant::now(),
                after: Duration::ZERO,
            },
            backoff: RETRY_FIRST,
        }
    }

    fn is_up(&self) -> bool {
        matches!(self.state, LiveState::Up { .. })
    }

    /// When to try reopening it, while it is down.
    fn retry_at(&self) -> Option<Instant> {
        match self.state {
            LiveState::Down { at, .. } => Some(at),
            LiveState::Connecting | LiveState::Up { .. } => None,
        }
    }

    fn connecting(&mut self) {
        self.state = LiveState::Connecting;
    }

    fn up(&mut self) {
        self.state = LiveState::Up {
            since: Instant::now(),
        };
    }

    /// Schedules a reopen, waiting longer each time unless it had been up a while.
    fn lost(&mut self) {
        if let LiveState::Up { since } = self.state
            && since.elapsed() >= STABLE_AFTER
        {
            self.backoff = RETRY_FIRST;
        }
        self.state = LiveState::Down {
            at: Instant::now() + self.backoff,
            after: self.backoff,
        };
        self.backoff = (self.backoff * 2).min(RETRY_MAX);
    }

    /// The wait chosen when it was last lost.
    fn retry_in(&self) -> Duration {
        match self.state {
            LiveState::Down { after, .. } => after,
            LiveState::Connecting | LiveState::Up { .. } => Duration::ZERO,
        }
    }
}

/// What the websocket said changed, gathered so a burst is fetched once.
#[derive(Default)]
struct Pending {
    /// The open conversation got a message.
    history: bool,
    /// A message already shown in it changed, so reload it whole.
    edited: bool,
    /// The open thread changed.
    thread: bool,
    /// Unread state moved in a conversation already listed.
    activity: bool,
    /// The list itself changed.
    relist: bool,
    /// New messages, to fetch when they are for the user and not shown already.
    arrivals: Vec<Arrival>,
}

impl Pending {
    fn any(&self) -> bool {
        self.history || self.thread || self.activity || self.relist || !self.arrivals.is_empty()
    }
}

/// A new message the websocket announced, and where.
#[derive(Clone, Debug)]
struct Arrival {
    channel: String,
    thread_ts: Option<String>,
    posted: Posted,
}

impl Arrival {
    /// The thread it replies in; `None` for a message in the conversation itself.
    fn reply_to(&self) -> Option<&str> {
        self.thread_ts
            .as_deref()
            .filter(|thread_ts| *thread_ts != self.posted.ts)
    }
}

/// Tells the websocket thread to stop once the worker is gone.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// The thread being viewed, and what of it was last sent.
struct OpenThread {
    channel: String,
    thread_ts: String,
    sent: Vec<Message>,
}

/// The background side. Generic over the transport so tests drive it directly.
struct Worker<H: Http> {
    api: SlackApi<H>,
    events: Sender<Event>,
    /// The conversation being viewed, polled for new messages.
    open: Option<String>,
    /// The thread being viewed, polled for new replies.
    thread: Option<OpenThread>,
    /// Latest message ts applied per conversation.
    seen: HashMap<String, String>,
    /// Latest reply ts sent per thread, by conversation and parent ts.
    replies_seen: HashMap<(String, String), String>,
    /// Users whose names have already been sent, or were known from the cache.
    known_users: HashSet<String>,
    /// Everything known about those users, for `People` without a lookup.
    users: HashMap<String, User>,
    /// When conversations were last listed.
    listed: Instant,
    /// Our own user id, once `auth.test` has answered.
    me: Option<String>,
    /// Every conversation last listed, before keeping the sidebar's.
    channels: Vec<Channel>,
    live: Live,
    pending: Pending,
}

impl<H: Http> Worker<H> {
    fn new(api: SlackApi<H>, events: Sender<Event>) -> Self {
        Self {
            api,
            events,
            open: None,
            thread: None,
            seen: HashMap::new(),
            replies_seen: HashMap::new(),
            known_users: HashSet::new(),
            users: HashMap::new(),
            listed: Instant::now(),
            me: None,
            channels: Vec::new(),
            live: Live::new(),
            pending: Pending::default(),
        }
    }

    /// Starts out knowing `users`, so they are not looked up again.
    fn knowing(mut self, users: Vec<User>) -> Self {
        for user in users {
            self.known_users.insert(user.id.clone());
            self.users.insert(user.id.clone(), user);
        }
        self
    }

    /// Serves `commands` until told to stop. `live` reaches the same queue, for
    /// the websocket to report changes on.
    fn run(mut self, commands: Receiver<Command>, live: Sender<Command>) {
        if !self.connect() {
            return;
        }
        let stop = StopOnDrop(Arc::new(AtomicBool::new(false)));
        self.handle(Command::Refresh);
        self.fetch_emoji();
        // A deadline rather than a timeout, so commands cannot keep pushing polls back.
        let mut polled = Instant::now();
        loop {
            if self.live.retry_at().is_some_and(|at| at <= Instant::now()) {
                self.start_live(&live, &stop.0);
            }
            let next_poll = polled + self.poll_interval();
            let mut wake = self
                .live
                .retry_at()
                .map_or(next_poll, |at| at.min(next_poll));
            // A change whose fetch failed is tried again soon, not at the next poll.
            if self.pending.any() {
                wake = wake.min(Instant::now() + POLL_INTERVAL);
            }
            match commands.recv_timeout(wake.saturating_duration_since(Instant::now())) {
                Ok(command) => {
                    // Take everything already queued before fetching anything.
                    let mut next = Some(command);
                    while let Some(command) = next {
                        if command == Command::Stop {
                            return;
                        }
                        self.handle(command);
                        next = commands.try_recv().ok();
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.apply_pending();
            if polled.elapsed() >= self.poll_interval() {
                self.poll();
                polled = Instant::now();
            }
        }
    }

    fn poll_interval(&self) -> Duration {
        if self.live.is_up() {
            LIVE_POLL_INTERVAL
        } else {
            POLL_INTERVAL
        }
    }

    /// Opens the websocket, which reports back as [`Command::Live`] on `live`.
    fn start_live(&mut self, live: &Sender<Command>, stop: &Arc<AtomicBool>) {
        match self.api.rtm_connect() {
            Ok(url) => {
                self.live.connecting();
                let live = live.clone();
                socket::spawn(
                    url,
                    self.api.creds().cookie.clone(),
                    Arc::clone(stop),
                    move |change| live.send(Command::Live(change)).is_ok(),
                );
            }
            Err(err) => {
                if is_expiry(&err) {
                    self.report(&err);
                }
                self.live.lost();
                eprintln!(
                    "meow: slack: live connection unavailable ({err}); retrying in {}s",
                    self.live.retry_in().as_secs()
                );
            }
        }
    }

    /// Notes what a websocket change means to fetch; [`Self::apply_pending`] does it.
    fn on_change(&mut self, change: Change) {
        match change {
            Change::Hello => {
                eprintln!("meow: slack: live connection up");
                self.live.up();
                // Whatever happened while it was down was only polled for.
                self.pending.history = self.open.is_some();
                self.pending.thread = self.thread.is_some();
                self.pending.activity = true;
            }
            Change::Lost => {
                self.live.lost();
                eprintln!(
                    "meow: slack: live connection down; polling, retrying in {}s",
                    self.live.retry_in().as_secs()
                );
            }
            Change::Message {
                channel,
                thread_ts,
                edited,
                posted,
            } => {
                if let Some(posted) = posted {
                    self.pending.arrivals.push(Arrival {
                        channel: channel.clone(),
                        thread_ts: thread_ts.clone(),
                        posted,
                    });
                }
                if self.open.as_deref() == Some(&channel) {
                    self.pending.history = true;
                    self.pending.edited |= edited;
                }
                if let (Some(thread), Some(thread_ts)) = (&self.thread, &thread_ts)
                    && thread.channel == channel
                    && &thread.thread_ts == thread_ts
                {
                    self.pending.thread = true;
                }
                self.activity_in(&channel);
            }
            Change::Read { channel } => self.activity_in(&channel),
            Change::Conversations => self.pending.relist = true,
        }
    }

    /// Unread state moved in `channel`: refresh it, or relist for one not listed yet.
    fn activity_in(&mut self, channel: &str) {
        if self.channels.iter().any(|c| c.id == channel) {
            self.pending.activity = true;
        } else {
            self.pending.relist = true;
        }
    }

    /// Fetches what the websocket said changed since last time. What failed
    /// for a passing reason stays pending, to be tried again.
    fn apply_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        let mut retry = Pending::default();
        if pending.relist {
            let result = self.try_refresh();
            retry.relist = self.should_retry(result);
        } else if pending.activity {
            let result = self.refresh_activity();
            retry.activity = self.should_retry(result);
        }
        if pending.history {
            let result = self.fetch_open(pending.edited);
            retry.history = self.should_retry(result);
            retry.edited = retry.history && pending.edited;
        }
        if pending.thread {
            let result = self.refresh_thread();
            retry.thread = self.should_retry(result);
        }
        // After a relist, so a conversation just joined is known.
        retry.arrivals = self.fetch_arrivals(pending.arrivals, retry.relist);
        self.pending = retry;
    }

    /// Fetches the new messages for the user that nothing else fetches (in
    /// conversations other than the open one, and in threads), so they reach
    /// Activity and Autopilot. Returns those to try again, with those in a
    /// conversation not listed yet while `relisting` is still to be done.
    fn fetch_arrivals(&mut self, arrivals: Vec<Arrival>, relisting: bool) -> Vec<Arrival> {
        let (unlisted, arrivals): (Vec<Arrival>, Vec<Arrival>) = arrivals
            .into_iter()
            .partition(|a| relisting && self.kind_of(&a.channel).is_none());
        let mut wanted: Vec<Arrival> = arrivals.into_iter().filter(|a| self.wants(a)).collect();
        // One fetch per conversation or thread, from its earliest new message.
        wanted.sort_by(|a, b| {
            (&a.channel, a.reply_to(), &a.posted.ts).cmp(&(&b.channel, b.reply_to(), &b.posted.ts))
        });
        wanted.dedup_by(|later, first| {
            later.channel == first.channel && later.reply_to() == first.reply_to()
        });
        let failed: Vec<Arrival> = wanted
            .into_iter()
            .filter(|arrival| {
                let result = self.fetch_arrival(arrival);
                self.should_retry(result)
            })
            .collect();
        unlisted.into_iter().chain(failed).collect()
    }

    /// Whether to fetch `arrival`, which [`Self::fetch_open`] does not: someone
    /// else wrote in a DM or group DM or mentioned the user, or the user
    /// answered where such a message was sent (so the app sees it answered).
    fn wants(&self, arrival: &Arrival) -> bool {
        let (Some(me), Some(author)) = (self.me.as_deref(), arrival.posted.user.as_deref()) else {
            return false;
        };
        if arrival.reply_to().is_none() && self.open.as_deref() == Some(&arrival.channel) {
            return false;
        }
        if author == me {
            return self.sent_before(arrival);
        }
        match self.kind_of(&arrival.channel) {
            Some(ChannelKind::Im | ChannelKind::Mpim) => true,
            Some(ChannelKind::Channel) | None => arrival.posted.text.contains(&format!("<@{me}>")),
        }
    }

    /// Whether messages of `arrival`'s conversation or thread were sent already.
    fn sent_before(&self, arrival: &Arrival) -> bool {
        match arrival.reply_to() {
            Some(thread_ts) => self
                .replies_seen
                .contains_key(&(arrival.channel.clone(), thread_ts.to_string())),
            None => self.seen.contains_key(&arrival.channel),
        }
    }

    /// The kind of a listed conversation.
    fn kind_of(&self, channel: &str) -> Option<ChannelKind> {
        self.channels
            .iter()
            .find(|c| c.id == channel)
            .map(|c| c.kind)
    }

    /// Sends what is new from `arrival` on: a thread's replies, or a
    /// conversation's messages after its whole history. Nothing is marked read.
    fn fetch_arrival(&mut self, arrival: &Arrival) -> Result<(), ApiError> {
        let channel = arrival.channel.clone();
        let since = arrival.posted.ts.as_str();
        if let Some(thread_ts) = arrival.reply_to() {
            let thread = self.api.replies(&channel, thread_ts)?;
            let key = (channel.clone(), thread_ts.to_string());
            let last_sent = self.replies_seen.get(&key).map(String::as_str);
            let fresh: Vec<Message> = thread
                .iter()
                .filter(|m| m.ts.as_str() >= since && m.ts != thread_ts)
                .filter(|m| last_sent.is_none_or(|last| m.ts.as_str() > last))
                .cloned()
                .collect();
            if let Some(last) = fresh.last() {
                self.replies_seen.insert(key, last.ts.clone());
                self.resolve_users(&authors(&thread));
                self.emit(Event::Replies {
                    channel,
                    thread,
                    fresh,
                });
            }
            return Ok(());
        }
        let messages = self.api.history(&channel, HISTORY_LIMIT)?;
        let fresh: Vec<Message> = self
            .since_seen(&channel, messages.clone())
            .into_iter()
            .filter(|m| m.ts.as_str() >= since)
            .collect();
        self.remember_latest(&channel, &messages);
        if !fresh.is_empty() {
            self.resolve_users(&authors(&messages));
            self.emit(Event::History {
                channel: channel.clone(),
                messages,
            });
            self.emit(Event::New {
                channel,
                messages: fresh,
            });
        }
        Ok(())
    }

    /// Whether a failed fetch is worth trying again; a fatal failure is reported instead.
    fn should_retry(&self, result: Result<(), ApiError>) -> bool {
        match result {
            Ok(()) => false,
            Err(err) if err.retriable() => true,
            Err(err) => {
                self.report(&err);
                false
            }
        }
    }

    /// Confirms the session and announces who we are. Returns whether to carry on.
    fn connect(&mut self) -> bool {
        match self.api.auth_test() {
            Ok(auth) => {
                self.me = Some(auth.user_id.clone());
                self.emit(Event::Connected(auth))
            }
            Err(err) => self.fail(&err),
        }
    }

    /// Custom emoji are a nicety: without them, names stay as typed.
    fn fetch_emoji(&mut self) {
        match self.api.emoji_list() {
            Ok(emoji) => {
                self.emit(Event::CustomEmoji(emoji));
            }
            Err(err) => eprintln!("meow: slack: custom emoji unavailable: {err}"),
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Refresh => self.refresh(),
            Command::Open(channel) => self.open(channel),
            Command::OpenThread { channel, thread_ts } => self.open_thread(channel, thread_ts),
            Command::CloseThread => self.thread = None,
            Command::Send {
                channel,
                text,
                thread_ts,
            } => self.send(channel, text, thread_ts),
            Command::FetchMyMessages { limit } => self.fetch_my_messages(limit),
            Command::People(channel) => self.people(channel),
            Command::Live(change) => self.on_change(change),
            Command::Stop => {}
        }
    }

    fn refresh(&mut self) {
        if let Err(err) = self.try_refresh() {
            self.report(&err);
        }
    }

    fn try_refresh(&mut self) -> Result<(), ApiError> {
        let channels = self.api.all_conversations(TYPES)?;
        self.listed = Instant::now();
        self.channels = channels.clone();
        // Without activity, show every conversation rather than none.
        let channels = match self.api.counts() {
            Ok(activity) if !activity.is_empty() => with_activity(channels, &activity),
            _ => channels,
        };
        self.show(channels);
        Ok(())
    }

    /// Fresh unread state for the conversations already listed, without
    /// listing them all again.
    fn refresh_activity(&mut self) -> Result<(), ApiError> {
        if self.channels.is_empty() {
            return self.try_refresh();
        }
        let activity = self.api.counts()?;
        if !activity.is_empty() {
            self.show(with_activity(self.channels.clone(), &activity));
        }
        Ok(())
    }

    /// Sends the conversations to show, then the names of their people.
    fn show(&mut self, channels: Vec<Channel>) {
        let others: Vec<String> = channels.iter().filter_map(|c| c.user.clone()).collect();
        self.emit(Event::Conversations(channels));
        self.resolve_users(&others);
    }

    fn open(&mut self, channel: String) {
        self.thread = None;
        match self.api.history(&channel, HISTORY_LIMIT) {
            Ok(messages) => {
                self.remember_latest(&channel, &messages);
                self.mark_read(&channel, &messages);
                let authors = authors(&messages);
                self.open = Some(channel.clone());
                self.emit(Event::History { channel, messages });
                self.resolve_users(&authors);
            }
            Err(err) => self.report(&err),
        }
    }

    fn open_thread(&mut self, channel: String, thread_ts: String) {
        self.thread = Some(OpenThread {
            channel,
            thread_ts,
            sent: Vec::new(),
        });
        if let Err(err) = self.refresh_thread() {
            self.report(&err);
        }
    }

    /// Sends the open thread when it changed since last sent.
    fn refresh_thread(&mut self) -> Result<(), ApiError> {
        let Some(thread) = &self.thread else {
            return Ok(());
        };
        let messages = self.api.replies(&thread.channel, &thread.thread_ts)?;
        // Nothing back (a deleted thread) leaves what is shown alone.
        if messages.is_empty() || messages == thread.sent {
            return Ok(());
        }
        let event = Event::Thread {
            channel: thread.channel.clone(),
            thread_ts: thread.thread_ts.clone(),
            messages: messages.clone(),
        };
        let authors = authors(&messages);
        if let Some(thread) = &mut self.thread {
            thread.sent = messages;
        }
        self.emit(event);
        self.resolve_users(&authors);
        Ok(())
    }

    fn send(&mut self, channel: String, text: String, thread_ts: Option<String>) {
        match self.api.post_message(&channel, &text, thread_ts.as_deref()) {
            Ok(()) => {
                self.emit(Event::Sent {
                    channel: channel.clone(),
                });
                // Pick up our own message (and anything else) right away; a
                // failure here is caught up by the next poll.
                if thread_ts.is_some() {
                    let _ = self.refresh_thread();
                }
                if self.open.as_deref() == Some(&channel) {
                    let _ = self.fetch_open(false);
                }
            }
            Err(err) => self.report(&err),
        }
    }

    fn people(&mut self, channel: String) {
        let people = self
            .api
            .members(&channel)
            .and_then(|ids| {
                ids.iter()
                    .take(MAX_PEOPLE)
                    .map(|id| self.user(id))
                    .collect::<Result<Vec<User>, ApiError>>()
            })
            .map_err(|err| err.to_string());
        self.emit(Event::People { channel, people });
    }

    /// A user, from what is already known or else looked up.
    fn user(&mut self, id: &str) -> Result<User, ApiError> {
        if let Some(user) = self.users.get(id) {
            return Ok(user.clone());
        }
        let user = self.api.user_info(id)?;
        self.users.insert(id.to_string(), user.clone());
        Ok(user)
    }

    fn fetch_my_messages(&self, limit: usize) {
        let result = match &self.me {
            Some(me) => self
                .api
                .messages_from(me, limit)
                .map_err(|err| err.to_string()),
            None => Err("not connected to Slack yet".to_string()),
        };
        self.emit(Event::MyMessages(result));
    }

    /// Idle work: new messages in the open conversation, and an occasional relist.
    fn poll(&mut self) {
        if self.listed.elapsed() >= RELIST_EVERY {
            self.refresh();
        }
        // A failed poll is silent unless the session died; the next one retries.
        let result = self.refresh_thread();
        self.should_retry(result);
        let result = self.fetch_open(false);
        self.should_retry(result);
    }

    /// Sends what is new in the open conversation, and with `reload` all of it
    /// again too, for edits and deletions.
    fn fetch_open(&mut self, reload: bool) -> Result<(), ApiError> {
        let Some(channel) = self.open.clone() else {
            return Ok(());
        };
        let messages = self.api.history(&channel, HISTORY_LIMIT)?;
        let all = reload.then(|| messages.clone());
        let fresh = self.since_seen(&channel, messages);
        if !fresh.is_empty() {
            self.remember_latest(&channel, &fresh);
            self.mark_read(&channel, &fresh);
            let authors = authors(&fresh);
            self.emit(Event::New {
                channel: channel.clone(),
                messages: fresh,
            });
            self.resolve_users(&authors);
        }
        if let Some(messages) = all {
            self.emit(Event::History { channel, messages });
        }
        Ok(())
    }

    /// Keeps only messages newer than the last one applied for `channel`.
    fn since_seen(&self, channel: &str, messages: Vec<Message>) -> Vec<Message> {
        match self.seen.get(channel) {
            Some(last) => messages.into_iter().filter(|m| &m.ts > last).collect(),
            None => messages,
        }
    }

    fn remember_latest(&mut self, channel: &str, messages: &[Message]) {
        if let Some(last) = messages.last() {
            self.seen.insert(channel.to_string(), last.ts.clone());
        }
    }

    fn mark_read(&self, channel: &str, messages: &[Message]) {
        if let Some(last) = messages.last() {
            let _ = self.api.mark(channel, &last.ts);
        }
    }

    /// Looks up users not seen before and sends their names in one batch.
    fn resolve_users(&mut self, ids: &[String]) {
        let mut users = Vec::new();
        for id in ids {
            if id.is_empty() || !self.known_users.insert(id.clone()) {
                continue;
            }
            if let Ok(user) = self.api.user_info(id) {
                self.users.insert(id.clone(), user.clone());
                users.push(user);
            }
        }
        if !users.is_empty() {
            self.emit(Event::Users(users));
        }
    }

    /// Turns an API failure into an event: expiry, or a one-off error.
    fn report(&self, err: &ApiError) {
        let _ = self.fail(err);
    }

    fn fail(&self, err: &ApiError) -> bool {
        if is_expiry(err) {
            self.emit(Event::Expired)
        } else {
            self.emit(Event::Error(err.to_string()))
        }
    }

    fn emit(&self, event: Event) -> bool {
        self.events.send(event).is_ok()
    }
}

/// Keeps the conversations Slack's sidebar shows, with their unread state,
/// most recent first. Users accumulate hundreds of stale direct messages; the
/// sidebar's few dozen are the ones that matter.
fn with_activity(channels: Vec<Channel>, activity: &[Activity]) -> Vec<Channel> {
    let by_id: HashMap<&str, &Activity> = activity.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut shown: Vec<Channel> = channels
        .into_iter()
        .filter_map(|channel| {
            let activity = by_id.get(channel.id.as_str())?;
            Some(Channel {
                latest_ts: activity.latest.clone().or(channel.latest_ts.clone()),
                unread: activity.unread,
                mentions: activity.mentions,
                ..channel
            })
        })
        .collect();
    shown.sort_by(|a, b| b.latest_ts.cmp(&a.latest_ts));
    shown
}

/// The distinct, non-empty authors of some messages.
fn authors(messages: &[Message]) -> Vec<String> {
    let mut seen = HashSet::new();
    messages
        .iter()
        .filter_map(|m| m.user.clone())
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

/// Whether the error means the session is no longer valid.
fn is_expiry(err: &ApiError) -> bool {
    use super::model::SlackError;
    matches!(
        err,
        ApiError::Slack(SlackError::Api(code))
            if matches!(code.as_str(),
                "invalid_auth" | "not_authed" | "token_revoked" | "token_expired" | "account_inactive")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::http::{Http, Response};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Answers each Slack method from its own queue, so a `mark` or `users.info`
    /// never steals a `conversations.history` body. Unqueued methods reply `ok`.
    struct Router {
        by_method: RefCell<HashMap<String, Vec<String>>>,
        /// Every method called, in order.
        calls: RefCell<Vec<String>>,
    }

    impl Router {
        fn new() -> Self {
            Self {
                by_method: RefCell::new(HashMap::new()),
                calls: RefCell::new(Vec::new()),
            }
        }

        /// How many times `method` was called.
        fn count(&self, method: &str) -> usize {
            self.calls.borrow().iter().filter(|m| *m == method).count()
        }

        fn on(self, method: &str, bodies: &[&str]) -> Self {
            self.by_method.borrow_mut().insert(
                method.to_string(),
                bodies.iter().map(|s| s.to_string()).collect(),
            );
            self
        }
    }

    impl Http for Router {
        fn post_form(&self, url: &str, _: &str, _: &[(&str, &str)]) -> Result<Response, String> {
            let method = url.rsplit('/').next().unwrap_or_default();
            self.calls.borrow_mut().push(method.to_string());
            let body = self
                .by_method
                .borrow_mut()
                .get_mut(method)
                .filter(|queue| !queue.is_empty())
                .map(|queue| queue.remove(0))
                .unwrap_or_else(|| r#"{"ok":true}"#.to_string());
            // "!500" answers with that HTTP status instead.
            if let Some(status) = body.strip_prefix('!') {
                let status = status.parse().expect("an HTTP status");
                return Ok(Response {
                    status,
                    body: String::new(),
                });
            }
            Ok(Response { status: 200, body })
        }
    }

    fn creds() -> Credentials {
        Credentials {
            token: "xoxc".into(),
            cookie: "xoxd".into(),
            team_id: "T1".into(),
            domain: "d".into(),
            user_id: "U1".into(),
        }
    }

    fn worker(router: Router) -> (Worker<Router>, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (Worker::new(SlackApi::new(router, creds()), tx), rx)
    }

    fn events(rx: &Receiver<Event>) -> Vec<Event> {
        rx.try_iter().collect()
    }

    const BOB: &str = r#"{"ok":true,"user":{"id":"U2","profile":{"display_name":"bob"}}}"#;

    fn channel(id: &str, user: &str) -> Channel {
        Channel {
            id: id.into(),
            kind: crate::slack::model::ChannelKind::Im,
            name: String::new(),
            user: Some(user.into()),
            latest_ts: None,
            unread: false,
            mentions: 0,
        }
    }

    #[test]
    fn activity_keeps_sidebar_conversations_newest_first() {
        let channels = vec![
            channel("D1", "U1"),
            channel("D2", "U2"),
            channel("D3", "U3"),
        ];
        let activity = [
            Activity {
                id: "D1".into(),
                latest: Some("1.0".into()),
                unread: false,
                mentions: 0,
            },
            Activity {
                id: "D3".into(),
                latest: Some("9.0".into()),
                unread: true,
                mentions: 2,
            },
        ];
        let shown = with_activity(channels, &activity);
        let ids: Vec<&str> = shown.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["D3", "D1"]);
        assert!(shown[0].unread);
        assert_eq!(shown[0].mentions, 2);
        assert_eq!(shown[0].latest_ts.as_deref(), Some("9.0"));
    }

    #[test]
    fn refresh_resolves_only_shown_conversations() {
        let list = r#"{"ok":true,"channels":[
            {"id":"D1","is_im":true,"user":"U2"},{"id":"D2","is_im":true,"user":"U3"}]}"#;
        let counts = r#"{"ok":true,"ims":[{"id":"D1","latest":"1.0"}]}"#;
        let (mut worker, rx) = worker(
            Router::new()
                .on("users.conversations", &[list])
                .on("client.counts", &[counts])
                .on("users.info", &[BOB]),
        );
        worker.refresh();
        match events(&rx).as_slice() {
            [Event::Conversations(shown), Event::Users(users)] => {
                assert_eq!(shown.len(), 1);
                assert_eq!(users.len(), 1);
            }
            other => panic!("{other:?}"),
        }
        assert!(!worker.known_users.contains("U3"));
    }

    #[test]
    fn connect_emits_identity() {
        let (mut worker, rx) =
            worker(Router::new().on("auth.test", &[r#"{"ok":true,"user_id":"U1"}"#]));
        assert!(worker.connect());
        assert!(matches!(events(&rx).as_slice(), [Event::Connected(_)]));
    }

    #[test]
    fn people_are_the_members_with_their_profiles() {
        let members = r#"{"ok":true,"members":["U2"]}"#;
        let bob = r#"{"ok":true,"user":{"id":"U2","tz":"Europe/Paris",
            "profile":{"display_name":"bob","email":"bob@example.com"}}}"#;
        let (mut worker, rx) = worker(
            Router::new()
                .on("conversations.members", &[members])
                .on("users.info", &[bob]),
        );
        worker.handle(Command::People("G1".into()));
        match events(&rx).as_slice() {
            [
                Event::People {
                    channel,
                    people: Ok(people),
                },
            ] => {
                assert_eq!(channel, "G1");
                assert_eq!(people[0].email, "bob@example.com");
                assert_eq!(people[0].tz, "Europe/Paris");
            }
            other => panic!("{other:?}"),
        }
    }

    fn cached_bob() -> User {
        User {
            id: "U2".into(),
            real_name: "Bob".into(),
            display_name: "bob".into(),
            email: "bob@example.com".into(),
            tz: "Europe/Paris".into(),
        }
    }

    #[test]
    fn people_known_from_the_cache_are_not_looked_up() {
        let list = r#"{"ok":true,"channels":[{"id":"D1","is_im":true,"user":"U2"}]}"#;
        let (worker, rx) = worker(Router::new().on("users.conversations", &[list]));
        let mut worker = worker.knowing(vec![cached_bob()]);
        // No users.info is queued: a lookup would fail and send no name.
        worker.refresh();
        assert!(matches!(events(&rx).as_slice(), [Event::Conversations(_)]));
    }

    #[test]
    fn people_in_a_conversation_come_from_the_cache_when_known() {
        let members = r#"{"ok":true,"members":["U2"]}"#;
        let (worker, rx) = worker(Router::new().on("conversations.members", &[members]));
        let mut worker = worker.knowing(vec![cached_bob()]);
        worker.handle(Command::People("G1".into()));
        match events(&rx).as_slice() {
            [
                Event::People {
                    people: Ok(people), ..
                },
            ] => assert_eq!(people, &[cached_bob()]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn my_messages_are_searched_as_the_connected_user() {
        let search = r#"{"ok":true,"messages":{"paging":{"page":1,"pages":1},
            "matches":[{"text":"on it"},{"text":"done"}]}}"#;
        let (mut worker, rx) = worker(
            Router::new()
                .on("auth.test", &[r#"{"ok":true,"user_id":"U1"}"#])
                .on("search.messages", &[search]),
        );
        worker.handle(Command::FetchMyMessages { limit: 10 });
        assert_eq!(
            events(&rx),
            vec![Event::MyMessages(Err("not connected to Slack yet".into()))]
        );
        worker.connect();
        let _ = events(&rx);
        worker.handle(Command::FetchMyMessages { limit: 10 });
        assert_eq!(
            events(&rx),
            vec![Event::MyMessages(Ok(vec!["on it".into(), "done".into()]))]
        );
    }

    #[test]
    fn connect_reports_expiry_on_invalid_auth() {
        let (mut worker, rx) =
            worker(Router::new().on("auth.test", &[r#"{"ok":false,"error":"invalid_auth"}"#]));
        worker.connect();
        assert_eq!(events(&rx), vec![Event::Expired]);
    }

    #[test]
    fn open_loads_history_then_resolves_authors() {
        let history = r#"{"ok":true,"messages":[{"ts":"1.0","user":"U2","text":"hi"}]}"#;
        let (mut worker, rx) = worker(
            Router::new()
                .on("conversations.history", &[history])
                .on("users.info", &[BOB]),
        );
        worker.open("D1".into());
        let got = events(&rx);
        assert!(matches!(got[0], Event::History { .. }));
        match &got[1] {
            Event::Users(users) => assert_eq!(users[0].name(), "bob"),
            other => panic!("{other:?}"),
        }
        assert_eq!(worker.open.as_deref(), Some("D1"));
        assert_eq!(worker.seen.get("D1").map(String::as_str), Some("1.0"));
    }

    #[test]
    fn poll_emits_only_messages_newer_than_seen() {
        let first = r#"{"ok":true,"messages":[{"ts":"1.0","user":"U2","text":"one"}]}"#;
        let second = r#"{"ok":true,"messages":[
            {"ts":"2.0","user":"U2","text":"two"},{"ts":"1.0","user":"U2","text":"one"}]}"#;
        let (mut worker, rx) = worker(Router::new().on("conversations.history", &[first, second]));
        worker.known_users.insert("U2".into()); // skip user_info lookups
        worker.open("D1".into());
        let _ = events(&rx);
        worker.poll();
        let got = events(&rx);
        match got.iter().find(|e| matches!(e, Event::New { .. })) {
            Some(Event::New { messages, .. }) => {
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].text, "two");
            }
            _ => panic!("expected one new message, got {got:?}"),
        }
        assert_eq!(worker.seen.get("D1").map(String::as_str), Some("2.0"));
    }

    #[test]
    fn poll_without_new_messages_is_quiet() {
        let same = r#"{"ok":true,"messages":[{"ts":"1.0","user":"U2","text":"one"}]}"#;
        let (mut worker, rx) = worker(Router::new().on("conversations.history", &[same, same]));
        worker.known_users.insert("U2".into());
        worker.open("D1".into());
        let _ = events(&rx);
        worker.poll();
        assert!(!events(&rx).iter().any(|e| matches!(e, Event::New { .. })));
    }

    const THREAD: &str = r#"{"ok":true,"messages":[
        {"ts":"1.0","user":"U1","text":"parent","thread_ts":"1.0","reply_count":1},
        {"ts":"2.0","user":"U2","text":"reply","thread_ts":"1.0"}]}"#;
    const LONGER_THREAD: &str = r#"{"ok":true,"messages":[
        {"ts":"1.0","user":"U1","text":"parent","thread_ts":"1.0","reply_count":2},
        {"ts":"2.0","user":"U2","text":"reply","thread_ts":"1.0"},
        {"ts":"3.0","user":"U2","text":"another","thread_ts":"1.0"}]}"#;

    fn open_thread() -> Command {
        Command::OpenThread {
            channel: "C1".into(),
            thread_ts: "1.0".into(),
        }
    }

    /// The messages of each `Thread` event, as texts.
    fn threads(events: &[Event]) -> Vec<Vec<String>> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Thread { messages, .. } => {
                    Some(messages.iter().map(|m| m.text.clone()).collect())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn opening_a_thread_sends_its_messages_then_resolves_authors() {
        let (mut worker, rx) = worker(
            Router::new()
                .on("conversations.replies", &[THREAD])
                .on("users.info", &[BOB]),
        );
        worker.known_users.insert("U1".into());
        worker.handle(open_thread());
        let got = events(&rx);
        match got.as_slice() {
            [
                Event::Thread {
                    channel,
                    thread_ts,
                    messages,
                },
                Event::Users(users),
            ] => {
                assert_eq!((channel.as_str(), thread_ts.as_str()), ("C1", "1.0"));
                assert_eq!(messages.len(), 2);
                assert_eq!(users[0].name(), "bob");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_open_thread_is_sent_again_only_when_it_changes() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.replies", &[THREAD, THREAD, LONGER_THREAD]));
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        worker.handle(open_thread());
        worker.poll();
        worker.poll();
        assert_eq!(
            threads(&events(&rx)),
            [vec!["parent", "reply"], vec!["parent", "reply", "another"]]
        );
    }

    #[test]
    fn a_closed_thread_is_no_longer_polled() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.replies", &[THREAD, LONGER_THREAD]));
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        worker.handle(open_thread());
        worker.handle(Command::CloseThread);
        worker.poll();
        assert_eq!(threads(&events(&rx)).len(), 1);
    }

    #[test]
    fn opening_a_conversation_leaves_the_thread() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.replies", &[THREAD, LONGER_THREAD]));
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        worker.handle(open_thread());
        worker.handle(Command::Open("D1".into()));
        worker.poll();
        assert_eq!(threads(&events(&rx)).len(), 1);
    }

    fn live_message(channel: &str, thread_ts: Option<&str>, edited: bool) -> Change {
        Change::Message {
            channel: channel.into(),
            thread_ts: thread_ts.map(Into::into),
            edited,
            posted: None,
        }
    }

    /// The texts of the first `New` event, if any.
    fn new_texts(events: &[Event]) -> Option<Vec<String>> {
        events.iter().find_map(|event| match event {
            Event::New { messages, .. } => Some(messages.iter().map(|m| m.text.clone()).collect()),
            _ => None,
        })
    }

    const ONE: &str = r#"{"ok":true,"messages":[{"ts":"1.0","user":"U2","text":"one"}]}"#;
    const ONE_TWO: &str = r#"{"ok":true,"messages":[
        {"ts":"2.0","user":"U2","text":"two"},{"ts":"1.0","user":"U2","text":"one"}]}"#;
    const ONE_EDITED: &str =
        r#"{"ok":true,"messages":[{"ts":"1.0","user":"U2","text":"one, fixed"}]}"#;

    #[test]
    fn a_live_message_in_the_open_conversation_is_shown() {
        let (mut worker, rx) = worker(Router::new().on("conversations.history", &[ONE, ONE_TWO]));
        worker.known_users.insert("U2".into());
        worker.open("D1".into());
        let _ = events(&rx);
        worker.on_change(live_message("D1", None, false));
        worker.apply_pending();
        assert_eq!(new_texts(&events(&rx)), Some(vec!["two".to_string()]));
    }

    #[test]
    fn a_burst_of_live_changes_fetches_once() {
        let (mut worker, rx) = worker(Router::new().on("conversations.history", &[ONE, ONE_TWO]));
        worker.known_users.insert("U2".into());
        worker.open("D1".into());
        for _ in 0..3 {
            worker.on_change(live_message("D1", None, false));
        }
        worker.apply_pending();
        worker.apply_pending();
        let _ = events(&rx);
        assert_eq!(worker.api.http().count("conversations.history"), 2);
    }

    #[test]
    fn a_live_edit_reloads_the_open_conversation() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.history", &[ONE, ONE_EDITED]));
        worker.known_users.insert("U2".into());
        worker.open("D1".into());
        let _ = events(&rx);
        worker.on_change(live_message("D1", None, true));
        worker.apply_pending();
        let got = events(&rx);
        match got.iter().find(|e| matches!(e, Event::History { .. })) {
            Some(Event::History { messages, .. }) => assert_eq!(messages[0].text, "one, fixed"),
            _ => panic!("expected the history again, got {got:?}"),
        }
        assert_eq!(new_texts(&got), None);
    }

    #[test]
    fn a_live_reply_refreshes_the_open_thread() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.replies", &[THREAD, LONGER_THREAD]));
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        worker.handle(open_thread());
        worker.on_change(live_message("C1", Some("1.0"), false));
        worker.apply_pending();
        assert_eq!(
            threads(&events(&rx)),
            [vec!["parent", "reply"], vec!["parent", "reply", "another"]]
        );
    }

    #[test]
    fn a_reply_in_another_thread_leaves_the_open_one_alone() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.replies", &[THREAD, LONGER_THREAD]));
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        worker.handle(open_thread());
        worker.on_change(live_message("C1", Some("7.0"), false));
        worker.apply_pending();
        assert_eq!(threads(&events(&rx)).len(), 1);
    }

    const TWO_IMS: &str = r#"{"ok":true,"channels":[
        {"id":"D1","is_im":true,"user":"U2"},{"id":"D2","is_im":true,"user":"U3"}]}"#;
    const BOTH_READ: &str =
        r#"{"ok":true,"ims":[{"id":"D1","latest":"1.0"},{"id":"D2","latest":"1.0"}]}"#;
    const D2_UNREAD: &str = r#"{"ok":true,"ims":[{"id":"D1","latest":"1.0"},
        {"id":"D2","latest":"5.0","has_unreads":true}]}"#;

    #[test]
    fn a_live_message_elsewhere_refreshes_unread_state_only() {
        let (worker, rx) = worker(
            Router::new()
                .on("users.conversations", &[TWO_IMS])
                .on("client.counts", &[BOTH_READ, D2_UNREAD]),
        );
        let mut worker = worker.knowing(vec![cached_bob()]);
        worker.known_users.insert("U3".into());
        worker.refresh();
        let _ = events(&rx);
        worker.on_change(live_message("D2", None, false));
        worker.apply_pending();
        match events(&rx).as_slice() {
            [Event::Conversations(shown)] => {
                assert_eq!(shown[0].id, "D2");
                assert!(shown[0].unread);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(worker.api.http().count("users.conversations"), 1);
        assert_eq!(worker.api.http().count("conversations.history"), 0);
    }

    #[test]
    fn a_message_in_an_unknown_conversation_relists() {
        let (mut worker, rx) = worker(Router::new().on("users.conversations", &[TWO_IMS, TWO_IMS]));
        worker
            .known_users
            .extend(["U2".to_string(), "U3".to_string()]);
        worker.refresh();
        let _ = events(&rx);
        worker.on_change(live_message("D9", None, false));
        worker.apply_pending();
        assert_eq!(worker.api.http().count("users.conversations"), 2);
    }

    fn live_post(
        channel: &str,
        thread_ts: Option<&str>,
        ts: &str,
        user: &str,
        text: &str,
    ) -> Change {
        Change::Message {
            channel: channel.into(),
            thread_ts: thread_ts.map(Into::into),
            edited: false,
            posted: Some(Posted {
                ts: ts.into(),
                user: Some(user.into()),
                text: text.into(),
            }),
        }
    }

    fn named_channel(id: &str) -> Channel {
        Channel {
            kind: crate::slack::model::ChannelKind::Channel,
            name: "eng".into(),
            user: None,
            ..channel(id, "")
        }
    }

    /// A worker signed in as U1, with a DM (D2) and a channel (C1) listed.
    fn signed_in(router: Router) -> (Worker<Router>, Receiver<Event>) {
        let (mut worker, rx) = worker(router);
        worker.me = Some("U1".into());
        worker.channels = vec![channel("D2", "U2"), named_channel("C1")];
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        (worker, rx)
    }

    #[test]
    fn a_message_for_me_elsewhere_is_fetched_without_marking_it_read() {
        let (mut worker, rx) = signed_in(Router::new().on("conversations.history", &[ONE_TWO]));
        worker.on_change(live_post("D2", None, "2.0", "U2", "two"));
        worker.apply_pending();
        let got = events(&rx);
        let history = got.iter().find_map(|event| match event {
            Event::History { channel, messages } if channel == "D2" => Some(messages.len()),
            _ => None,
        });
        assert_eq!(
            history,
            Some(2),
            "the conversation is cached whole: {got:?}"
        );
        assert_eq!(new_texts(&got), Some(vec!["two".to_string()]));
        assert_eq!(worker.api.http().count("conversations.mark"), 0);
    }

    #[test]
    fn a_message_already_sent_elsewhere_is_not_sent_again() {
        let (mut worker, rx) =
            signed_in(Router::new().on("conversations.history", &[ONE_TWO, ONE_TWO]));
        worker.on_change(live_post("D2", None, "2.0", "U2", "two"));
        worker.apply_pending();
        let _ = events(&rx);
        worker.on_change(live_post("D2", None, "2.0", "U2", "two"));
        worker.apply_pending();
        assert_eq!(new_texts(&events(&rx)), None);
    }

    #[test]
    fn a_channel_message_without_a_mention_is_not_fetched() {
        let (mut worker, _rx) = signed_in(Router::new());
        worker.on_change(live_post("C1", None, "2.0", "U2", "lunch anyone?"));
        worker.apply_pending();
        assert_eq!(worker.api.http().count("conversations.history"), 0);
    }

    #[test]
    fn a_mention_in_a_channel_is_fetched() {
        let (mut worker, rx) = signed_in(Router::new().on("conversations.history", &[ONE_TWO]));
        worker.on_change(live_post("C1", None, "2.0", "U2", "<@U1> can you look?"));
        worker.apply_pending();
        assert_eq!(new_texts(&events(&rx)), Some(vec!["two".to_string()]));
    }

    #[test]
    fn my_own_message_elsewhere_is_not_fetched() {
        let (mut worker, _rx) = signed_in(Router::new());
        worker.on_change(live_post("D2", None, "2.0", "U1", "on it"));
        worker.apply_pending();
        assert_eq!(worker.api.http().count("conversations.history"), 0);
    }

    #[test]
    fn a_message_in_the_open_conversation_is_fetched_once() {
        let (mut worker, rx) =
            signed_in(Router::new().on("conversations.history", &[ONE, ONE_TWO]));
        worker.open("D2".into());
        let _ = events(&rx);
        worker.on_change(live_post("D2", None, "2.0", "U2", "two"));
        worker.apply_pending();
        assert_eq!(new_texts(&events(&rx)), Some(vec!["two".to_string()]));
        assert_eq!(worker.api.http().count("conversations.history"), 2);
    }

    #[test]
    fn a_reply_for_me_comes_with_its_thread() {
        let (mut worker, rx) =
            signed_in(Router::new().on("conversations.replies", &[LONGER_THREAD]));
        worker.on_change(live_post("C1", Some("1.0"), "3.0", "U2", "<@U1> and this?"));
        worker.apply_pending();
        let got = events(&rx);
        match got.iter().find(|e| matches!(e, Event::Replies { .. })) {
            Some(Event::Replies {
                channel,
                thread,
                fresh,
            }) => {
                assert_eq!(channel, "C1");
                assert_eq!(thread.len(), 3);
                assert_eq!(fresh.len(), 1);
                assert_eq!(fresh[0].text, "another");
            }
            _ => panic!("expected the replies, got {got:?}"),
        }
        assert_eq!(new_texts(&got), None);
    }

    /// The `fresh` texts of each `Replies` event.
    fn fresh_replies(events: &[Event]) -> Vec<Vec<String>> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Replies { fresh, .. } => {
                    Some(fresh.iter().map(|m| m.text.clone()).collect())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn replies_already_sent_are_not_sent_again() {
        let (mut worker, rx) =
            signed_in(Router::new().on("conversations.replies", &[LONGER_THREAD, LONGER_THREAD]));
        worker.on_change(live_post("D2", Some("1.0"), "2.0", "U2", "reply"));
        worker.apply_pending();
        worker.on_change(live_post("D2", Some("1.0"), "3.0", "U2", "another"));
        worker.apply_pending();
        assert_eq!(fresh_replies(&events(&rx)), [vec!["reply", "another"]]);
    }

    #[test]
    fn my_answer_where_something_was_sent_is_sent_too() {
        let (mut worker, rx) =
            signed_in(Router::new().on("conversations.history", &[ONE, ONE_AND_MY_ANSWER]));
        worker.on_change(live_post("D2", None, "1.0", "U2", "one"));
        worker.apply_pending();
        let _ = events(&rx);
        worker.on_change(live_post("D2", None, "2.0", "U1", "done"));
        worker.apply_pending();
        assert_eq!(new_texts(&events(&rx)), Some(vec!["done".to_string()]));
    }

    #[test]
    fn my_answer_in_a_thread_already_sent_is_sent_too() {
        let (mut worker, rx) =
            signed_in(Router::new().on("conversations.replies", &[THREAD, THREAD_AND_MY_ANSWER]));
        worker.on_change(live_post("D2", Some("1.0"), "2.0", "U2", "reply"));
        worker.apply_pending();
        worker.on_change(live_post("D2", Some("1.0"), "3.0", "U1", "done"));
        worker.apply_pending();
        assert_eq!(fresh_replies(&events(&rx)), [vec!["reply"], vec!["done"]]);
    }

    #[test]
    fn names_are_known_before_the_messages_arrive() {
        let (mut worker, rx) = signed_in(
            Router::new()
                .on("conversations.history", &[ONE_TWO])
                .on("users.info", &[BOB]),
        );
        worker.known_users.remove("U2");
        worker.on_change(live_post("D2", None, "2.0", "U2", "two"));
        worker.apply_pending();
        let got = events(&rx);
        let users = got.iter().position(|e| matches!(e, Event::Users(_)));
        let new = got.iter().position(|e| matches!(e, Event::New { .. }));
        assert!(users < new, "names first: {got:?}");
    }

    #[test]
    fn a_message_in_a_conversation_not_listed_yet_waits_for_the_relist() {
        let (mut worker, rx) = signed_in(
            Router::new()
                .on("users.conversations", &["!500", WITH_D9])
                .on("conversations.history", &[ONE_TWO]),
        );
        worker.on_change(live_post("D9", None, "2.0", "U2", "two"));
        worker.apply_pending();
        assert!(worker.pending.any());
        worker.apply_pending();
        assert_eq!(new_texts(&events(&rx)), Some(vec!["two".to_string()]));
    }

    const ONE_AND_MY_ANSWER: &str = r#"{"ok":true,"messages":[
        {"ts":"2.0","user":"U1","text":"done"},{"ts":"1.0","user":"U2","text":"one"}]}"#;
    const THREAD_AND_MY_ANSWER: &str = r#"{"ok":true,"messages":[
        {"ts":"1.0","user":"U1","text":"parent","thread_ts":"1.0","reply_count":2},
        {"ts":"2.0","user":"U2","text":"reply","thread_ts":"1.0"},
        {"ts":"3.0","user":"U1","text":"done","thread_ts":"1.0"}]}"#;
    const WITH_D9: &str = r#"{"ok":true,"channels":[{"id":"D9","is_im":true,"user":"U2"}]}"#;

    #[test]
    fn a_failed_fetch_elsewhere_is_tried_again() {
        let (mut worker, rx) =
            signed_in(Router::new().on("conversations.history", &["!500", ONE_TWO]));
        worker.on_change(live_post("D2", None, "2.0", "U2", "two"));
        worker.apply_pending();
        assert!(worker.pending.any());
        worker.apply_pending();
        assert_eq!(new_texts(&events(&rx)), Some(vec!["two".to_string()]));
    }

    #[test]
    fn joining_a_conversation_relists() {
        let (mut worker, _rx) = worker(Router::new());
        worker.on_change(Change::Conversations);
        worker.apply_pending();
        assert_eq!(worker.api.http().count("users.conversations"), 1);
    }

    #[test]
    fn polling_slows_down_while_live_and_speeds_up_when_lost() {
        let (mut worker, _rx) = worker(Router::new());
        assert_eq!(worker.poll_interval(), POLL_INTERVAL);
        worker.on_change(Change::Hello);
        assert_eq!(worker.poll_interval(), LIVE_POLL_INTERVAL);
        worker.on_change(Change::Lost);
        assert_eq!(worker.poll_interval(), POLL_INTERVAL);
    }

    #[test]
    fn coming_back_live_catches_up_on_what_was_missed() {
        let (mut worker, rx) = worker(
            Router::new()
                .on("conversations.history", &[ONE, ONE_TWO])
                .on("conversations.replies", &[THREAD, LONGER_THREAD]),
        );
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        worker.open("D1".into());
        worker.handle(Command::OpenThread {
            channel: "D1".into(),
            thread_ts: "1.0".into(),
        });
        let _ = events(&rx);
        worker.on_change(Change::Hello);
        worker.apply_pending();
        let got = events(&rx);
        assert_eq!(new_texts(&got), Some(vec!["two".to_string()]));
        assert_eq!(threads(&got).len(), 1);
    }

    #[test]
    fn a_lost_connection_is_retried_later_and_later() {
        let (mut worker, _rx) = worker(Router::new());
        worker.on_change(Change::Lost);
        let first = worker.live.retry_in();
        worker.on_change(Change::Lost);
        let second = worker.live.retry_in();
        assert!(second > first, "{first:?} then {second:?}");
        // Up long enough to count as stable: start over from the first wait.
        worker.live.state = LiveState::Up {
            since: Instant::now() - STABLE_AFTER,
        };
        worker.on_change(Change::Lost);
        assert_eq!(worker.live.retry_in(), first);
    }

    #[test]
    fn a_connection_dropped_right_after_hello_keeps_backing_off() {
        let (mut worker, _rx) = worker(Router::new());
        worker.on_change(Change::Lost);
        let first = worker.live.retry_in();
        worker.on_change(Change::Hello);
        worker.on_change(Change::Lost);
        assert!(worker.live.retry_in() > first);
    }

    #[test]
    fn a_failed_live_fetch_is_kept_for_a_retry() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.history", &[ONE, "!503", ONE_EDITED]));
        worker.known_users.insert("U2".into());
        worker.open("D1".into());
        let _ = events(&rx);
        worker.on_change(live_message("D1", None, true));
        worker.apply_pending();
        assert!(
            events(&rx)
                .iter()
                .all(|e| !matches!(e, Event::History { .. }))
        );
        worker.apply_pending();
        let got = events(&rx);
        match got.iter().find(|e| matches!(e, Event::History { .. })) {
            Some(Event::History { messages, .. }) => assert_eq!(messages[0].text, "one, fixed"),
            _ => panic!("expected the edit on retry, got {got:?}"),
        }
    }

    #[test]
    fn a_fatal_live_fetch_is_reported_not_retried() {
        let (mut worker, rx) = worker(Router::new().on(
            "conversations.history",
            &[ONE, r#"{"ok":false,"error":"channel_not_found"}"#],
        ));
        worker.known_users.insert("U2".into());
        worker.open("D1".into());
        let _ = events(&rx);
        worker.on_change(live_message("D1", None, false));
        worker.apply_pending();
        assert!(matches!(events(&rx).as_slice(), [.., Event::Error(_)]));
        worker.apply_pending();
        assert_eq!(worker.api.http().count("conversations.history"), 2);
    }

    #[test]
    fn sending_a_thread_reply_refreshes_the_thread() {
        let (mut worker, rx) =
            worker(Router::new().on("conversations.replies", &[THREAD, LONGER_THREAD]));
        worker
            .known_users
            .extend(["U1".to_string(), "U2".to_string()]);
        worker.handle(open_thread());
        worker.send("C1".into(), "another".into(), Some("1.0".into()));
        assert_eq!(threads(&events(&rx)).len(), 2);
    }

    #[test]
    fn polling_relists_by_time_not_by_count() {
        let (mut worker, _rx) = worker(Router::new());
        worker.poll();
        assert_eq!(worker.api.http().count("users.conversations"), 0);
        worker.listed = Instant::now() - RELIST_EVERY;
        worker.poll();
        assert_eq!(worker.api.http().count("users.conversations"), 1);
    }

    #[test]
    fn send_reports_a_slack_error() {
        let (mut worker, rx) = worker(Router::new().on(
            "chat.postMessage",
            &[r#"{"ok":false,"error":"msg_too_long"}"#],
        ));
        worker.send("D1".into(), "x".into(), None);
        assert!(matches!(events(&rx).as_slice(), [Event::Error(_)]));
    }

    #[test]
    fn refresh_lists_conversations_and_resolves_im_partners() {
        let list = r#"{"ok":true,"channels":[{"id":"D1","is_im":true,"user":"U2"}]}"#;
        let (mut worker, rx) = worker(
            Router::new()
                .on("users.conversations", &[list])
                .on("users.info", &[BOB]),
        );
        worker.refresh();
        let got = events(&rx);
        assert!(matches!(&got[0], Event::Conversations(c) if c.len() == 1));
        assert!(matches!(&got[1], Event::Users(u) if u[0].name() == "bob"));
    }
}
