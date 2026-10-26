//! Custom Slack emoji and attached images on the app side. The worker sends
//! the workspace's emoji list once connected; images (emoji and thumbnails)
//! are downloaded only when they show up in an open conversation, into a
//! cache on disk that later launches reuse.

use crate::app::feature::Host;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use meow::slack::emoji::{self, Custom};
use meow::slack::model::Message;

use crate::ui::emoji_text::{EmojiGlyph, ImageGlyph};
use crate::ui::picture::Picture;

use super::Slack;
use super::downloads::{Download, Downloader};

/// Keys of image downloads start with this, emoji keys never do (`:` ends a name).
const IMAGE_KEY: &str = "file:";

#[derive(Default)]
pub(super) struct EmojiState {
    /// The workspace's custom emoji, by name.
    custom: HashMap<String, Custom>,
    /// What can be drawn now: images loaded, and aliases of standard emoji.
    pub(super) glyphs: HashMap<String, EmojiGlyph>,
    /// Attached images by file id, once seen; loaded when downloaded.
    pub(super) images: HashMap<String, ImageGlyph>,
    /// Keys already asked for (loaded, loading or failed): each is tried once.
    requested: HashSet<String>,
    downloader: Option<Downloader>,
}

impl Slack {
    /// The worker sent the workspace's custom emoji.
    pub(super) fn on_custom_emoji(&mut self, raw: &HashMap<String, String>) {
        let custom = emoji::resolve(raw);
        let standard = custom.iter().filter_map(|(name, value)| match value {
            Custom::Standard(text) => Some((name.clone(), EmojiGlyph::Text(text.clone()))),
            Custom::Image(_) => None,
        });
        self.emoji.glyphs.extend(standard);
        self.emoji.custom = custom;
        self.render_slack();
    }

    /// Fetches what `messages` need that is not loaded yet: custom emoji
    /// images, and attached images.
    pub(super) fn want_images(&mut self, messages: &[Message]) {
        let mut wanted = Vec::new();
        if let Some(dir) = emoji::cache_dir() {
            for message in messages {
                for (_, _, name) in emoji::shortcodes(&message.text) {
                    if let Some(Custom::Image(url)) = self.emoji.custom.get(name) {
                        wanted.push(Download {
                            key: name.to_string(),
                            url: url.clone(),
                            file: emoji::cache_file(&dir, name, url),
                            auth: None,
                        });
                    }
                }
            }
        }
        if let Some(dir) = files_dir() {
            for image in messages.iter().flat_map(Message::all_images) {
                self.emoji
                    .images
                    .entry(image.id.clone())
                    .or_insert_with(|| ImageGlyph {
                        image: None,
                        title: image.title.clone(),
                        link: image.permalink.clone(),
                    });
                wanted.push(Download {
                    key: format!("{IMAGE_KEY}{}", image.id),
                    url: image.thumb.clone(),
                    file: emoji::cache_file(&dir, &image.id, &image.thumb),
                    auth: self.slack.file_auth(),
                });
            }
        }
        wanted.retain(|download| self.emoji.requested.insert(download.key.clone()));
        if wanted.is_empty() {
            return;
        }
        let downloader = self.emoji.downloader.get_or_insert_with(Downloader::start);
        for download in wanted {
            downloader.request(download);
        }
    }

    /// Takes in finished downloads, and redraws the conversation once some are.
    pub(super) fn poll_emoji(&mut self, host: &mut dyn Host) {
        let Some(downloader) = &self.emoji.downloader else {
            return;
        };
        let finished = downloader.finished();
        if finished.is_empty() {
            return;
        }
        for (key, result) in finished {
            let image = result.and_then(|file| Picture::load(&file)).map(Rc::new);
            match (key.strip_prefix(IMAGE_KEY), image) {
                (Some(id), Ok(image)) => {
                    if let Some(glyph) = self.emoji.images.get_mut(id) {
                        glyph.image = Some(image);
                    }
                }
                (None, Ok(image)) => {
                    self.emoji.glyphs.insert(key, EmojiGlyph::Image(image));
                }
                (_, Err(error)) => eprintln!("meow: slack: {key} unavailable: {error}"),
            }
        }
        if self.slack_conversation_shown(host) {
            self.render_slack();
        }
    }

    /// Plays the animated emoji and images of the open conversation.
    pub(super) fn animate_slack(&self, host: &mut dyn Host) {
        if self.slack_conversation_shown(host) {
            self.page().animate();
        }
    }
}

/// `~/Library/Caches/Meow/slack-files`.
fn files_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches/Meow/slack-files"))
}
