//! Builds attributed strings for the transcript, including rendered Markdown.

use meow::markdown::{self, Span};
use meow::style;
use meow::transcript::Block;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, Message};
use objc2_app_kit::{
    NSBackgroundColorAttributeName, NSColor, NSCursor, NSCursorAttributeName, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSLinkAttributeName,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSStrikethroughStyleAttributeName,
    NSStrokeWidthAttributeName, NSTextBlock, NSTextBlockLayer, NSTextBlockValueType, NSTextView,
    NSUnderlineStyleAttributeName,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString,
    NSNumber, NSRange, NSRectEdge, NSString, NSURL,
};

use super::theme;

pub const BODY_SIZE: f64 = theme::BODY_SIZE;
const HEADING_SIZES: [f64; 3] = [24.0, 21.0, 19.0];
/// Stroke that thickens the pixel font into bold (negative: fill and stroke).
const HEAVY_STROKE: f64 = -3.0;

/// How one run of text looks.
pub struct Attributes {
    pub color: Retained<NSColor>,
    pub font: Retained<NSFont>,
    pub background: Option<Retained<NSColor>>,
    pub link: Option<Retained<NSURL>>,
    pub strike: bool,
    /// Bold, faked with a stroke since the pixel font has one weight.
    pub heavy: bool,
    pub underline: bool,
}

impl Attributes {
    pub fn plain(color: Retained<NSColor>, font: Retained<NSFont>) -> Self {
        Self {
            color,
            font,
            background: None,
            link: None,
            strike: false,
            heavy: false,
            underline: false,
        }
    }
}

pub fn attributed(text: &str, attributes: &Attributes) -> Retained<NSAttributedString> {
    let single_line = NSNumber::numberWithInteger(1);
    let stroke = NSNumber::numberWithDouble(HEAVY_STROKE);
    // SAFETY: AppKit's attribute-name constants are immutable statics.
    let mut keys: Vec<&NSAttributedStringKey> =
        unsafe { vec![NSForegroundColorAttributeName, NSFontAttributeName] };
    // The classic style's system font has a real bold, so no stroke is faked.
    let classic_bold = (attributes.heavy && style::is_classic())
        .then(|| NSFont::boldSystemFontOfSize(attributes.font.pointSize()));
    let heavy = attributes.heavy && classic_bold.is_none();
    let font = classic_bold.as_ref().unwrap_or(&attributes.font);
    let mut values: Vec<&AnyObject> = vec![&attributes.color, font];
    if let Some(background) = &attributes.background {
        keys.push(unsafe { NSBackgroundColorAttributeName });
        values.push(background);
    }
    if let Some(link) = &attributes.link {
        keys.push(unsafe { NSLinkAttributeName });
        values.push(link);
    }
    if attributes.strike {
        keys.push(unsafe { NSStrikethroughStyleAttributeName });
        values.push(&single_line);
    }
    if attributes.underline {
        keys.push(unsafe { NSUnderlineStyleAttributeName });
        values.push(&single_line);
    }
    if heavy {
        keys.push(unsafe { NSStrokeWidthAttributeName });
        values.push(&stroke);
    }
    let dictionary = NSDictionary::from_slices(&keys, &values);
    // SAFETY: every value has the type its attribute key expects.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&dictionary),
        )
    }
}

/// URL scheme of the clickable "N steps" line; the path is the turn's index.
const STEPS_SCHEME: &str = "meow-steps:";

/// The turn index behind a steps-line link, if `url` is one.
pub fn steps_turn(url: &str) -> Option<usize> {
    url.strip_prefix(STEPS_SCHEME)?.parse().ok()
}

/// URL scheme of a Slack message's clickable reply count; the path is the
/// message's index in the open conversation.
const THREAD_SCHEME: &str = "meow-thread:";

/// The link opening the thread of the open conversation's message `index`.
pub fn thread_link(index: usize) -> String {
    format!("{THREAD_SCHEME}{index}")
}

/// The message index behind a reply-count link, if `url` is one.
pub fn thread_index(url: &str) -> Option<usize> {
    url.strip_prefix(THREAD_SCHEME)?.parse().ok()
}

/// Links keep their own colors; hovering any link shows the hand cursor.
pub fn style_links(view: &NSTextView) {
    let cursor = NSCursor::pointingHandCursor();
    // SAFETY: AppKit's attribute-name constant is an immutable static.
    let keys = [unsafe { NSCursorAttributeName }];
    let values: [&AnyObject; 1] = [&cursor];
    // SAFETY: the cursor attribute holds an NSCursor, as AppKit expects.
    unsafe { view.setLinkTextAttributes(Some(&NSDictionary::from_slices(&keys, &values))) };
}

/// The whole conversation as styled text.
pub fn transcript(blocks: &[Block]) -> Retained<NSAttributedString> {
    let result = NSMutableAttributedString::new();
    for block in blocks {
        result.appendAttributedString(&block_text(block));
    }
    result.into_super()
}

fn block_text(block: &Block) -> Retained<NSAttributedString> {
    let line = |text: String, color: Retained<NSColor>, font: Retained<NSFont>| {
        attributed(&text, &Attributes::plain(color, font))
    };
    let small = || theme::font(theme::SMALL_SIZE);
    match block {
        Block::User(text) => boxed(
            &attributed(
                &format!("{text}\n"),
                &Attributes::plain(theme::ink(), theme::font(BODY_SIZE)),
            ),
            &theme::user_bubble(),
            Side::Right,
        ),
        Block::Info(text) => line(format!("{text}\n"), theme::muted(), small()),
        Block::Error(text) => line(
            format!("! {text}\n"),
            theme::error(),
            theme::font(BODY_SIZE),
        ),
        Block::Tool(text) => line(format!("    - {text}\n"), theme::muted(), small()),
        Block::Steps {
            turn,
            label,
            expanded,
        } => {
            let arrow = if *expanded { "▾" } else { "▸" };
            let url = NSURL::URLWithString(&NSString::from_str(&format!("{STEPS_SCHEME}{turn}")));
            attributed(
                &format!("{arrow} {label}\n"),
                &Attributes {
                    link: url,
                    ..Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE))
                },
            )
        }
        Block::Note(text) => markdown(text, theme::muted()),
        Block::Answer(text) => boxed(&markdown(text, theme::ink()), &theme::sidebar(), Side::Left),
    }
}

/// Which side of the transcript a message box keeps to.
enum Side {
    Left,
    Right,
}

/// Space inside a message box.
const BOX_PADDING: f64 = 10.0;
/// Space above and below a message box.
const BOX_GAP: f64 = 6.0;
/// The share of the width left free on the far side of a message box.
const BOX_OFFSET_PERCENT: f64 = 20.0;
/// The share of the width a message box's text takes (its padding comes on top).
const BOX_WIDTH_PERCENT: f64 = 76.0;

/// `text` (whole paragraphs) inside one padded box of `fill`, kept to `side`.
fn boxed(text: &NSAttributedString, fill: &NSColor, side: Side) -> Retained<NSAttributedString> {
    let block = NSTextBlock::new();
    block.setBackgroundColor(Some(fill));
    // The block paints its background over its margins too, so the space
    // around the box is a border in the page's own color, drawn on top.
    block.setBorderColor(Some(&theme::paper()));
    // Without a content width the block shrinks to nothing.
    block.setContentWidth_type(BOX_WIDTH_PERCENT, NSTextBlockValueType::PercentageValueType);
    block.setWidth_type_forLayer(
        BOX_PADDING,
        NSTextBlockValueType::AbsoluteValueType,
        NSTextBlockLayer::Padding,
    );
    for edge in [NSRectEdge::MinY, NSRectEdge::MaxY] {
        block.setWidth_type_forLayer_edge(
            BOX_GAP,
            NSTextBlockValueType::AbsoluteValueType,
            NSTextBlockLayer::Border,
            edge,
        );
    }
    let far = match side {
        Side::Left => NSRectEdge::MaxX,
        Side::Right => NSRectEdge::MinX,
    };
    block.setWidth_type_forLayer_edge(
        BOX_OFFSET_PERCENT,
        NSTextBlockValueType::PercentageValueType,
        NSTextBlockLayer::Border,
        far,
    );
    let style = NSMutableParagraphStyle::new();
    style.setTextBlocks(&NSArray::from_retained_slice(&[block]));
    let result = NSMutableAttributedString::initWithAttributedString(
        NSMutableAttributedString::alloc(),
        text,
    );
    // SAFETY: the paragraph-style attribute holds an NSParagraphStyle, as AppKit expects.
    unsafe {
        result.addAttribute_value_range(
            NSParagraphStyleAttributeName,
            &style,
            NSRange::new(0, result.length()),
        );
    }
    result.into_super()
}

/// Height of the blank line that separates runs of Slack messages.
const SLACK_GAP_SIZE: f64 = 8.0;

/// A Slack day divider: "── Today ──", set apart from the runs around it.
pub fn slack_day(label: &str) -> Retained<NSAttributedString> {
    let result = NSMutableAttributedString::new();
    result.appendAttributedString(&slack_gap());
    result.appendAttributedString(&attributed(
        &format!("── {label} ──\n"),
        &Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE)),
    ));
    result.into_super()
}

/// A run of Slack messages by one author: an initials chip, the name in bold
/// and the time once, then each body rendered from Markdown, indented past the chip.
/// One author's run of messages; body `focus`, if any, is marked.
pub fn slack_run(
    author: &str,
    time: &str,
    mine: bool,
    bodies: &[String],
    focus: Option<usize>,
) -> Retained<NSAttributedString> {
    let result = NSMutableAttributedString::new();
    result.appendAttributedString(&slack_gap());
    let chip = Attributes {
        background: Some(avatar_color(author, mine)),
        ..Attributes::plain(theme::on_accent(), theme::label_font(theme::LABEL_SIZE))
    };
    result.appendAttributedString(&attributed(&format!(" {} ", initials(author)), &chip));
    let name = Attributes {
        heavy: true,
        ..Attributes::plain(theme::ink(), theme::font(theme::SMALL_SIZE))
    };
    result.appendAttributedString(&attributed(&format!("  {author}"), &name));
    result.appendAttributedString(&attributed(
        &format!("  {time}\n"),
        &Attributes::plain(theme::muted(), theme::font(theme::SMALL_SIZE)),
    ));
    let style = NSMutableParagraphStyle::new();
    style.setFirstLineHeadIndent(SLACK_INDENT);
    style.setHeadIndent(SLACK_INDENT);
    for (index, body) in bodies.iter().enumerate() {
        let text = NSMutableAttributedString::initWithAttributedString(
            NSMutableAttributedString::alloc(),
            &markdown(body, theme::ink()),
        );
        // SAFETY: the paragraph-style attribute holds an NSParagraphStyle, and
        // the background one an NSColor, as AppKit expects.
        unsafe {
            text.addAttribute_value_range(
                NSParagraphStyleAttributeName,
                &style,
                NSRange::new(0, text.length()),
            );
            if focus == Some(index) {
                text.addAttribute_value_range(
                    NSBackgroundColorAttributeName,
                    &theme::field_border(),
                    NSRange::new(0, text.length()),
                );
            }
        }
        result.appendAttributedString(&text);
    }
    result.into_super()
}

/// How far message bodies sit from the left, past the initials chip.
const SLACK_INDENT: f64 = 40.0;

/// "HU" for Hugo, "IC" for Inès Clavel.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let letters: String = match words.as_slice() {
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
        [only] => only.chars().take(2).collect(),
        [] => "?".to_string(),
    };
    letters.to_uppercase()
}

/// A stable chip color per author; gold for the user.
fn avatar_color(name: &str, mine: bool) -> Retained<NSColor> {
    if mine {
        return theme::highlight();
    }
    let colors = [theme::link(), theme::live(), theme::accent(), theme::cyan()];
    let hash = name.bytes().fold(0usize, |h, b| {
        h.wrapping_mul(31).wrapping_add(usize::from(b))
    });
    colors[hash % colors.len()].clone()
}

/// Text in the private Claude panel: Markdown in the main ink color.
pub fn slack_assist_body(text: &str) -> Retained<NSAttributedString> {
    markdown(text, theme::ink())
}

fn slack_gap() -> Retained<NSAttributedString> {
    attributed(
        "\n",
        &Attributes::plain(theme::ink(), theme::font(SLACK_GAP_SIZE)),
    )
}

/// Markdown rendered for the transcript in `color`, ending with a newline.
fn markdown(text: &str, color: Retained<NSColor>) -> Retained<NSAttributedString> {
    let result = NSMutableAttributedString::new();
    for span in markdown::to_spans(text) {
        let attributes = span_attributes(&span, &color);
        result.appendAttributedString(&attributed(&span.text, &attributes));
    }
    let newline = Attributes::plain(color, theme::font(BODY_SIZE));
    result.appendAttributedString(&attributed("\n", &newline));
    result.into_super()
}

fn span_attributes(span: &Span, color: &NSColor) -> Attributes {
    let style = span.style;
    let size = match style.heading {
        0 => BODY_SIZE,
        level => HEADING_SIZES
            .get(usize::from(level) - 1)
            .copied()
            .unwrap_or(BODY_SIZE),
    };
    let font = theme::font(size);
    let link = span
        .link
        .as_deref()
        .and_then(|url| NSURL::URLWithString(&NSString::from_str(url)));
    Attributes {
        color: if link.is_some() {
            theme::link()
        } else if style.italic {
            theme::emphasis()
        } else {
            color.retain()
        },
        font,
        background: style.code.then(theme::code_background),
        link,
        strike: style.strike,
        heavy: style.bold || style.heading > 0,
        underline: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_links_round_trip_their_index() {
        assert_eq!(thread_index(&thread_link(12)), Some(12));
        assert_eq!(thread_index(&format!("{STEPS_SCHEME}3")), None);
        assert_eq!(thread_index("meow-thread:x"), None);
    }

    #[test]
    fn steps_links_round_trip_their_turn() {
        assert_eq!(steps_turn(&format!("{STEPS_SCHEME}7")), Some(7));
        assert_eq!(steps_turn("https://linear.app/x"), None);
        assert_eq!(steps_turn("meow-steps:abc"), None);
    }
}
