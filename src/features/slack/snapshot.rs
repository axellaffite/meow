//! Snapshot mode's Slack and Autopilot steps, with sample data: snapshots
//! never sign in to Slack, nor show the user's jobs.

use std::collections::HashMap;
use std::rc::Rc;

use meow::autopilot::{self, Jobs, Plan};
use meow::guard::Call;
use meow::sprites::{self, Bubble, Facing, Look, Pose};

use crate::app::feature::Host;
use crate::ui::autopilot_page;
use crate::ui::emoji_text::{self, ImageGlyph};
use crate::ui::picture::Picture;
use crate::ui::rich_text;
use crate::ui::sidebar::Page;
use crate::ui::slack_assist::AssistView;
use crate::ui::slack_page::{self, ConversationRow, TranscriptItem};
use crate::ui::widgets;

use super::{Slack, now_secs};

/// Each saved as `<step>.png`.
pub(super) const STEPS: [&str; 6] = [
    "autopilot",
    "slack-list",
    "slack-conversation",
    "slack-focus",
    "slack-thread",
    "slack-assist",
];

impl Slack {
    pub(super) fn show_sample(&mut self, step: &'static str, host: &mut dyn Host) {
        match step {
            "autopilot" => {
                host.open_page(Page::Feature(autopilot_page::PAGE_ID));
                self.autopilot_page()
                    .render(&sample_jobs(), true, now_secs());
                host.set_badge(autopilot_page::PAGE_ID, sample_jobs().waiting_count());
            }
            "slack-list" => {
                host.open_page(Page::Feature(slack_page::PAGE_ID));
                self.page().render_list(&sample_rows());
            }
            "slack-conversation" => self.page().render_conversation(
                false,
                "Hugo",
                &sample_transcript(),
                &Default::default(),
                &Default::default(),
            ),
            "slack-focus" => self.page().render_conversation(
                false,
                "Hugo",
                &focused_transcript(),
                &Default::default(),
                &Default::default(),
            ),
            "slack-thread" => self.page().render_conversation(
                true,
                "Thread in #release",
                &thread_transcript(),
                &Default::default(),
                &sample_images(),
            ),
            _ => {
                let attendees = ["toi".to_string(), "Hugo".to_string()];
                self.page().show_assist(&AssistView::Event {
                    question: "Cale un point avec Hugo demain",
                    note: "Vous êtes libres tous les deux demain à 10:00. Je propose :",
                    summary: "Point PR Android",
                    when: "mar. 20 oct · 10:00 – 10:30",
                    attendees: &attendees,
                    creating: false,
                });
            }
        }
    }
}

/// A job waiting for approval of a push and a reply, and one at work.
fn sample_jobs() -> Jobs {
    let mut jobs = Jobs::default();
    let origin = |ts: &str, text: &str| autopilot::Origin {
        channel: "D1".into(),
        ts: ts.into(),
        thread_ts: None,
        from: "Axel".into(),
        text: text.into(),
    };
    let plan = |title: &str| Plan {
        title: title.into(),
        prompt: String::new(),
        repo: None,
        pr: None,
        branch: None,
    };
    let now = now_secs();
    jobs.add(
        plan("Fix the conflicts on PR #42"),
        origin(
            "1",
            "Ta PR #42 a des conflits avec main, tu peux regarder ?",
        ),
        now,
    );
    jobs.finish(
        1,
        Ok("Merged `main` into `feat/window-size`: two conflicts in `src/ui/chat_window.rs`, \
            kept both sizes. `cargo test` passes. The push and a reply to Axel wait for you."
            .into()),
        vec![
            Call {
                tool: "Bash".into(),
                input: serde_json::json!({
                    "command": "git push fork HEAD:feat/window-size",
                    "description": "Push the merge to the PR branch",
                }),
            },
            Call {
                tool: "mcp__claude_ai_Slack__slack_send_message".into(),
                input: serde_json::json!({
                    "channel_id": "D1",
                    "message": "C'est réglé, j'ai mergé main et poussé, tu peux relancer la review.",
                }),
            },
        ],
        now,
    );
    jobs.add(
        plan("Find why the Android build fails"),
        origin("2", "Le build Android est rouge depuis ce matin"),
        now,
    );
    if let Some(job) = jobs.get_mut(2) {
        job.stage = autopilot::Stage::Working;
    }
    jobs
}

fn sample_rows() -> Vec<ConversationRow> {
    let row = |title: &str, unread: bool, mentions: u32, time: &str| ConversationRow {
        id: title.to_string(),
        title: title.to_string(),
        unread,
        mentions,
        time: time.to_string(),
        direct: !title.starts_with('#'),
    };
    vec![
        row("Hugo", true, 1, "09:42"),
        row("Inès", true, 0, "09:31"),
        row("Hugo, Inès", false, 0, "Mon"),
        row("Léa", false, 0, "Fri"),
        row("#general", true, 0, "10:02"),
    ]
}

fn sample_transcript() -> Vec<TranscriptItem> {
    let run = |author: &str, time: &str, mine: bool, bodies: &[&str]| TranscriptItem::Run {
        author: author.to_string(),
        time: time.to_string(),
        mine,
        bodies: bodies.iter().map(|b| b.to_string()).collect(),
        focus: None,
    };
    vec![
        TranscriptItem::Day("Today".to_string()),
        run(
            "Hugo",
            "09:41",
            false,
            &[&format!(
                "Tu aurais 30 min demain pour la PR Android ? Je préfère qu'on la passe ensemble.\n\
                 [↳ 2 replies]({})",
                rich_text::thread_link(0)
            )],
        ),
        run("Toi", "09:42", true, &["Oui, le matin m'arrange."]),
        run("Hugo", "09:42", false, &["Top, merci !"]),
        run(
            "Hugo",
            "09:44",
            false,
            &["Pour contexte :\n\
               > ↪ **Léa** in #android · [Open in Slack](https://example.slack.com/archives/C1/p1)\n\
               > La 3.2 part en review demain, il reste le fix du crash au lancement.\n\
               > 📎 crash-report.txt"],
        ),
    ]
}

/// A long conversation opened from Activity on one of Hugo's earlier messages.
fn focused_transcript() -> Vec<TranscriptItem> {
    let run =
        |author: &str, time: &str, bodies: &[&str], focus: Option<usize>| TranscriptItem::Run {
            author: author.to_string(),
            time: time.to_string(),
            mine: author == "Toi",
            bodies: bodies.iter().map(|b| b.to_string()).collect(),
            focus,
        };
    let mut items = vec![TranscriptItem::Day("Today".to_string())];
    for hour in 8..11 {
        items.push(run(
            "Inès",
            &format!("{hour:02}:05"),
            &["Le build de nuit est passé."],
            None,
        ));
        items.push(run("Toi", &format!("{hour:02}:20"), &["Merci !"], None));
    }
    items.push(run(
        "Hugo",
        "11:02",
        &[
            "Petite question sur la PR Android :",
            "@Toi tu peux relire le module paiement avant midi ?",
            "Sinon je décale la mise en prod.",
        ],
        Some(1),
    ));
    for hour in 12..16 {
        items.push(run(
            "Inès",
            &format!("{hour:02}:10"),
            &["Point café ?"],
            None,
        ));
        items.push(run("Léa", &format!("{hour:02}:30"), &["Go"], None));
    }
    items
}

/// A thread opened from #release: the parent, then replies with a screenshot.
fn thread_transcript() -> Vec<TranscriptItem> {
    let run = |author: &str, time: &str, bodies: &[String]| TranscriptItem::Run {
        author: author.to_string(),
        time: time.to_string(),
        mine: author == "Toi",
        bodies: bodies.to_vec(),
        focus: None,
    };
    vec![
        TranscriptItem::Day("Today".to_string()),
        run(
            "Inès",
            "10:02",
            &["Le build iOS est rouge, quelqu'un regarde ?".to_string()],
        ),
        TranscriptItem::Day("2 replies".to_string()),
        run(
            "Hugo",
            "10:05",
            &[
                "Je regarde, voilà ce que j'ai :".to_string(),
                emoji_text::image_marker("F1"),
            ],
        ),
        run("Toi", "10:07", &["Merci, je relance la CI.".to_string()]),
    ]
}

/// The thread's screenshot: the cat, standing in for a real image.
fn sample_images() -> HashMap<String, ImageGlyph> {
    let look = Look {
        pose: Pose::Stand,
        bubble: Bubble::None,
        facing: Facing::Right,
    };
    let frame = sprites::render(&look, Default::default());
    let glyph = ImageGlyph {
        image: Some(Rc::new(Picture::new(widgets::pixel_image(&frame, 8.0)))),
        title: "build.png".to_string(),
        link: None,
    };
    HashMap::from([("F1".to_string(), glyph)])
}
