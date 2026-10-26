//! The live connection: Slack's RTM websocket, opened with the session token's
//! `rtm.connect` URL and the `d` cookie, the way the web client does.
//!
//! Frames are only nudges. Each one says which conversation changed; the
//! worker then fetches it through the Web API, so messages are parsed in one
//! place and a missed frame costs nothing the fallback poll cannot catch up.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message as WsMessage, WebSocket};

/// What a frame says changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The connection is up and Slack is sending events.
    Hello,
    /// The connection ended; polling carries on until it is back.
    Lost,
    /// A message arrived, was edited or deleted, or got a reply.
    Message {
        channel: String,
        /// The thread it belongs to, when it is a reply.
        thread_ts: Option<String>,
        /// Whether a message already shown changed, rather than a new one.
        edited: bool,
        /// The message itself, when it is a new one.
        posted: Option<Posted>,
    },
    /// The user read a conversation somewhere, so its unread state moved.
    Read { channel: String },
    /// The user joined, left or opened a conversation.
    Conversations,
}

/// A new message as the frame shows it: enough to tell whether it is for the
/// user before fetching anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Posted {
    pub ts: String,
    pub user: Option<String>,
    /// Raw `mrkdwn`, mentions as `<@U123>`.
    pub text: String,
}

/// The change a text frame announces, if any Meow shows.
pub fn parse(text: &str) -> Option<Change> {
    let frame: Value = serde_json::from_str(text).ok()?;
    let channel = || frame["channel"].as_str().map(str::to_string);
    match frame["type"].as_str()? {
        "hello" => Some(Change::Hello),
        "goodbye" => Some(Change::Lost),
        "message" => {
            let edited = matches!(
                frame["subtype"].as_str(),
                Some("message_changed" | "message_deleted" | "message_replied")
            );
            // An edit carries the message inside; a deletion, what it was.
            let thread_ts = [&frame, &frame["message"], &frame["previous_message"]]
                .into_iter()
                .find_map(|m| m["thread_ts"].as_str())
                .map(str::to_string);
            let posted = frame["ts"].as_str().filter(|_| !edited).map(|ts| Posted {
                ts: ts.to_string(),
                user: frame["user"].as_str().map(str::to_string),
                text: frame["text"].as_str().unwrap_or_default().to_string(),
            });
            Some(Change::Message {
                channel: channel()?,
                thread_ts,
                edited,
                posted,
            })
        }
        "channel_marked" | "group_marked" | "im_marked" | "mpim_marked" => Some(Change::Read {
            channel: channel()?,
        }),
        "channel_joined" | "channel_left" | "group_joined" | "group_left" | "im_created"
        | "im_open" | "im_close" | "mpim_joined" | "mpim_open" | "mpim_close" => {
            Some(Change::Conversations)
        }
        _ => None,
    }
}

/// How long to wait for the TCP connection and the websocket handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a read blocks before the loop checks whether to stop or ping.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How often to ping Slack, which keeps the connection and its proxies awake.
const PING_EVERY: Duration = Duration::from_secs(20);
/// Without any frame for this long (pongs included), the connection is dead:
/// two pings went unanswered.
const SILENCE_LIMIT: Duration = Duration::from_secs(45);
/// A wall-clock gap this long between two reads means the Mac slept, which
/// [`Instant`] does not count, and the connection is probably gone.
const SLEEP_GAP: Duration = Duration::from_secs(30);

/// Opens `url` with the `d` cookie on a thread of its own and calls `on_change`
/// for each change, then once with [`Change::Lost`] when the connection ends.
/// It also ends when `on_change` returns false or `stop` is set.
pub fn spawn<F>(url: String, cookie: String, stop: Arc<AtomicBool>, on_change: F)
where
    F: Fn(Change) -> bool + Send + 'static,
{
    thread::spawn(move || {
        if let Err(why) = listen(&url, &cookie, &stop, &on_change) {
            eprintln!("meow: slack: live connection lost: {why}");
        }
        on_change(Change::Lost);
    });
}

fn listen(
    url: &str,
    cookie: &str,
    stop: &AtomicBool,
    on_change: &dyn Fn(Change) -> bool,
) -> Result<(), String> {
    let (mut socket, tcp) = open(url, cookie)?;
    tcp.set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|e| e.to_string())?;
    // A ping into a stalled connection must not hang the thread either.
    tcp.set_write_timeout(Some(READ_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut awake = SystemTime::now();
    let mut heard = Instant::now();
    let mut pinged = Instant::now();
    let mut ping_id: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        match socket.read() {
            Ok(WsMessage::Text(text)) => {
                heard = Instant::now();
                match parse(text.as_str()) {
                    Some(Change::Lost) => return Ok(()),
                    Some(change) => {
                        if !on_change(change) {
                            return Ok(());
                        }
                    }
                    None => {}
                }
            }
            Ok(WsMessage::Close(_)) => return Ok(()),
            Ok(_) => heard = Instant::now(),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.to_string()),
        }
        if heard.elapsed() > SILENCE_LIMIT {
            return Err("Slack went quiet".to_string());
        }
        let now = SystemTime::now();
        if now.duration_since(awake).unwrap_or_default() > SLEEP_GAP {
            return Err("woke from sleep".to_string());
        }
        awake = now;
        if pinged.elapsed() >= PING_EVERY {
            ping_id += 1;
            let ping = format!(r#"{{"id":{ping_id},"type":"ping"}}"#);
            socket
                .send(WsMessage::text(ping))
                .map_err(|e| e.to_string())?;
            pinged = Instant::now();
        }
    }
    let _ = socket.close(None);
    Ok(())
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Connects and shakes hands. Also returns a handle on the TCP stream, to
/// shorten its read timeout once connected.
fn open(url: &str, cookie: &str) -> Result<(Socket, TcpStream), String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    let cookie = format!("d={cookie}")
        .parse()
        .map_err(|_| "the session cookie is not a valid header".to_string())?;
    request.headers_mut().insert("Cookie", cookie);
    let host = request
        .uri()
        .host()
        .ok_or_else(|| "no host in the websocket URL".to_string())?;
    let port = request.uri().port_u16().unwrap_or(443);
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| format!("{host} did not resolve"))?;
    let tcp = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let handle = tcp.try_clone().map_err(|e| e.to_string())?;
    let (socket, _) = tungstenite::client_tls(request, tcp).map_err(|e| e.to_string())?;
    Ok((socket, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(channel: &str, thread_ts: Option<&str>, edited: bool) -> Option<Change> {
        Some(Change::Message {
            channel: channel.into(),
            thread_ts: thread_ts.map(Into::into),
            edited,
            posted: None,
        })
    }

    fn posted(channel: &str, thread_ts: Option<&str>, posted: Posted) -> Option<Change> {
        Some(Change::Message {
            channel: channel.into(),
            thread_ts: thread_ts.map(Into::into),
            edited: false,
            posted: Some(posted),
        })
    }

    #[test]
    fn hello_and_goodbye_frame_the_connection() {
        assert_eq!(parse(r#"{"type":"hello"}"#), Some(Change::Hello));
        assert_eq!(parse(r#"{"type":"goodbye"}"#), Some(Change::Lost));
    }

    #[test]
    fn a_new_message_says_who_wrote_what() {
        let frame = r#"{"type":"message","channel":"D1","user":"U2","text":"hi","ts":"2.0"}"#;
        let hi = Posted {
            ts: "2.0".into(),
            user: Some("U2".into()),
            text: "hi".into(),
        };
        assert_eq!(parse(frame), posted("D1", None, hi));
    }

    #[test]
    fn a_reply_names_its_thread() {
        let frame = r#"{"type":"message","channel":"C1","ts":"3.0","thread_ts":"1.0"}"#;
        let reply = Posted {
            ts: "3.0".into(),
            user: None,
            text: String::new(),
        };
        assert_eq!(parse(frame), posted("C1", Some("1.0"), reply));
    }

    #[test]
    fn an_edit_is_found_inside_the_changed_message() {
        let frame = r#"{"type":"message","subtype":"message_changed","channel":"C1",
            "message":{"ts":"3.0","thread_ts":"1.0","text":"fixed"}}"#;
        assert_eq!(parse(frame), message("C1", Some("1.0"), true));
    }

    #[test]
    fn a_deletion_is_found_in_the_previous_message() {
        let frame = r#"{"type":"message","subtype":"message_deleted","channel":"D1",
            "previous_message":{"ts":"2.0"}}"#;
        assert_eq!(parse(frame), message("D1", None, true));
    }

    #[test]
    fn a_new_reply_updates_its_parent() {
        let frame = r#"{"type":"message","subtype":"message_replied","channel":"C1",
            "message":{"ts":"1.0","thread_ts":"1.0","reply_count":3}}"#;
        assert_eq!(parse(frame), message("C1", Some("1.0"), true));
    }

    #[test]
    fn reading_elsewhere_moves_unread_state() {
        let frame = r#"{"type":"im_marked","channel":"D1","ts":"2.0"}"#;
        assert_eq!(
            parse(frame),
            Some(Change::Read {
                channel: "D1".into()
            })
        );
    }

    #[test]
    fn joining_a_conversation_relists() {
        assert_eq!(
            parse(r#"{"type":"im_created","channel":{"id":"D9"}}"#),
            Some(Change::Conversations)
        );
        assert_eq!(
            parse(r#"{"type":"channel_left","channel":"C1"}"#),
            Some(Change::Conversations)
        );
    }

    #[test]
    fn frames_meow_does_not_show_are_ignored() {
        assert_eq!(parse(r#"{"type":"user_typing","channel":"D1"}"#), None);
        assert_eq!(parse(r#"{"type":"pong","reply_to":1}"#), None);
        assert_eq!(parse(r#"{"ok":true,"reply_to":1}"#), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"type":"message","text":"no channel"}"#), None);
    }
}
