//! One background thread that downloads small files (custom emoji, Slack
//! image thumbnails) into a cache on disk, so the tick never waits on the
//! network. A file already cached is not downloaded again.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

/// Emoji and thumbnails are small; anything bigger is cut off.
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
/// The only host the Slack session is ever sent to.
const SLACK_FILES_HOST: &str = "https://files.slack.com/";

/// The Slack session, for files behind it.
#[derive(Clone)]
pub(super) struct SlackAuth {
    pub token: String,
    pub cookie: String,
}

pub(super) struct Download {
    /// What the caller knows the file by (an emoji name, a file id).
    pub key: String,
    pub url: String,
    pub file: PathBuf,
    /// Sent only when `url` is on Slack's file host.
    pub auth: Option<SlackAuth>,
}

/// A finished download: its key and the cached file, or why not.
pub(super) type Downloaded = (String, Result<PathBuf, String>);

pub(super) struct Downloader {
    requests: Sender<Download>,
    results: Receiver<Downloaded>,
}

impl Downloader {
    pub(super) fn start() -> Self {
        let (requests, incoming) = mpsc::channel::<Download>();
        let (results, outgoing) = mpsc::channel();
        thread::spawn(move || {
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(TIMEOUT))
                .build()
                .new_agent();
            for download in incoming {
                let result = fetch(&agent, &download);
                if results.send((download.key, result)).is_err() {
                    return;
                }
            }
        });
        Self {
            requests,
            results: outgoing,
        }
    }

    pub(super) fn request(&self, download: Download) {
        let _ = self.requests.send(download);
    }

    /// The downloads finished since last asked.
    pub(super) fn finished(&self) -> Vec<Downloaded> {
        let mut done = Vec::new();
        loop {
            match self.results.try_recv() {
                Ok(result) => done.push(result),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return done,
            }
        }
    }
}

/// The cached file, downloading it first if needed.
fn fetch(agent: &ureq::Agent, download: &Download) -> Result<PathBuf, String> {
    let file = &download.file;
    if file.exists() {
        return Ok(file.clone());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut request = agent.get(&download.url);
    if let Some(auth) = download
        .auth
        .as_ref()
        .filter(|_| sends_session(&download.url))
    {
        request = request
            .header("Authorization", format!("Bearer {}", auth.token))
            .header("Cookie", format!("d={}", auth.cookie));
    }
    let mut response = request.call().map_err(|e| e.to_string())?;
    let is_image = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|kind| kind.starts_with("image/"));
    if !is_image {
        // Slack answers a missing session with its sign-in page.
        return Err("not an image (signed out?)".to_string());
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    write_atomically(file, &bytes)?;
    Ok(file.clone())
}

fn sends_session(url: &str) -> bool {
    url.starts_with(SLACK_FILES_HOST)
}

fn write_atomically(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = file.with_extension("part");
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_goes_only_to_slack_files() {
        assert!(sends_session(
            "https://files.slack.com/files-tmb/T1-F1/a_480.png"
        ));
        assert!(!sends_session(
            "https://emoji.slack-edge.com/T1/parrot/a.gif"
        ));
        assert!(!sends_session("https://files.slack.com.evil.example/x.png"));
        assert!(!sends_session("http://files.slack.com/x.png"));
    }
}
