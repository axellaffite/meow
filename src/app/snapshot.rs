//! Debug snapshots of the bubble, for checking the look without screen access.
//!
//! With `MEOW_SNAPSHOT_DIR` set, Meow opens the bubble a few seconds after
//! launch, shows each page in turn, saves it as a PNG in that folder, then
//! quits. The Slack pages are filled with sample messages, so no sign-in is
//! needed (launch with dummy `MEOW_SLACK_*` values to keep off the Keychain).

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use meow::autopilot::{self, Jobs, Plan};
use meow::guard::Call;
use meow::sprites::{self, Bubble, Facing, Look, Pose};
use meow::transcript::Transcript;

use crate::ui::emoji_text::{self, ImageGlyph};
use crate::ui::events::UiEvent;
use crate::ui::picture::Picture;
use crate::ui::rich_text;
use crate::ui::slack_assist::AssistView;
use crate::ui::slack_page::{ConversationRow, TranscriptItem};
use crate::ui::widgets;

use super::{App, now_secs};

/// The pages, in order; each is saved once it has settled.
const STEPS: [&str; 12] = [
    "chat",
    "chat-two-lines",
    "chat-long",
    "tasks",
    "tasks-reply",
    "plan",
    "autopilot",
    "slack-list",
    "slack-conversation",
    "slack-focus",
    "slack-thread",
    "slack-assist",
];
/// Time for the app to settle before the first step (fonts, tasks).
const START_DELAY: Duration = Duration::from_secs(4);
const STEP_DELAY: Duration = Duration::from_millis(2500);

pub(super) struct Snapshot {
    dir: PathBuf,
    started: Instant,
    /// Index of the next step to start; a step's file is saved before the next one.
    next: usize,
}

impl Snapshot {
    pub(super) fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("MEOW_SNAPSHOT_DIR")?);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("meow: snapshot folder {}: {error}", dir.display());
            return None;
        }
        Some(Self {
            dir,
            started: Instant::now(),
            next: 0,
        })
    }
}

impl App {
    pub(super) fn poll_snapshot(&mut self) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let next = snapshot.next;
        if snapshot.started.elapsed() < START_DELAY + STEP_DELAY * next as u32 {
            return;
        }
        if next == 0 && !self.chat.is_visible() {
            self.handle(UiEvent::ToggleChat);
        }
        if next > 0 {
            self.save_snapshot(STEPS[next - 1]);
        }
        if next == STEPS.len() {
            eprintln!("meow: snapshots saved");
            std::process::exit(0);
        }
        self.show_snapshot_step(STEPS[next]);
        if let Some(snapshot) = self.snapshot.as_mut() {
            snapshot.next += 1;
        }
    }

    fn show_snapshot_step(&mut self, step: &str) {
        match step {
            "chat" => {
                self.handle(UiEvent::ShowChat);
                self.chat.chat().render(&sample_chat(), true);
            }
            "chat-two-lines" => self.chat.chat().set_draft(
                "Rédige une relance pour la relecture Android, en restant court et sympa avec Hugo.",
            ),
            "chat-long" => self.chat.chat().set_draft(
                "Rédige une relance pour la relecture Android.\nReste court et sympa.\n\
                 Mentionne que la mise en prod attend cette PR.\nEt propose un créneau demain.",
            ),
            "tasks" => {
                self.chat.chat().set_draft("");
                self.handle(UiEvent::ShowTasks);
            }
            "tasks-reply" => {
                // The first task asks whether to draft a reply; never ticked for real.
                if let Some(id) = self.chat.tasks().id_at(0) {
                    self.pending_done = Some((id, Instant::now() + Duration::from_secs(3600)));
                    self.tasks_changed();
                }
            }
            "plan" => self.handle(UiEvent::QuickAction(0)),
            "autopilot" => {
                self.handle(UiEvent::ShowAutopilot);
                self.chat.render_autopilot(&sample_jobs(), true, now_secs());
            }
            "slack-list" => {
                self.handle(UiEvent::ShowSlack);
                self.chat.slack().render_list(&sample_rows());
            }
            "slack-conversation" => {
                self.chat
                    .slack()
                    .render_conversation(
                        false,
                        "Hugo",
                        &sample_transcript(),
                        &Default::default(),
                        &Default::default(),
                    );
            }
            "slack-focus" => {
                self.chat.slack().render_conversation(
                    false,
                    "Hugo",
                    &focused_transcript(),
                    &Default::default(),
                    &Default::default(),
                );
            }
            "slack-thread" => {
                self.chat.slack().render_conversation(
                    true,
                    "Thread in #release",
                    &thread_transcript(),
                    &Default::default(),
                    &sample_images(),
                );
            }
            _ => {
                let attendees = ["toi".to_string(), "Hugo".to_string()];
                self.chat.slack().show_assist(&AssistView::Event {
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

    fn save_snapshot(&self, name: &str) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let path = snapshot.dir.join(format!("{name}.png"));
        match self.chat.snapshot(&path) {
            Ok(()) => eprintln!("meow: saved {}", path.display()),
            Err(error) => eprintln!("meow: snapshot {name}: {error}"),
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

fn sample_chat() -> Transcript {
    let mut chat = Transcript::default();
    chat.info("Click the cat or press ⌥⇧Space to talk. Right-click for options.");
    chat.start_turn("Tu peux résumer le fil #release de ce matin ?");
    chat.tool("slack_read_channel #release");
    chat.tool("slack_read_thread");
    chat.text(
        "Voici l'essentiel :\n\n- build iOS **validée**\n- la PR Android attend une relecture\n\
         - mise en prod après le standup",
    );
    chat.finish_turn("");
    chat.start_turn("Rédige une relance pour la relecture Android.");
    chat.text("Proposition :\n\n> Petite relance sur la PR Android : elle bloque la mise en prod.");
    chat.finish_turn("");
    chat
}
