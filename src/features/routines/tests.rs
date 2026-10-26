use std::time::{Duration, Instant};

use meow::conversations::ChatId;
use meow::routines::Routines;

use super::{Msg, PAGE_ID, ScheduledTasks};
use crate::app::feature::Feature;
use crate::features::testing::FakeHost;

const STAPLES: &str = "I'll tell you.\n```meow-schedule\n{\"name\": \"Staples\", \"kind\": \
    \"when\", \"condition\": \"Staples replied\", \"then\": \"Tell me\", \
    \"notify_only\": true}\n```";

/// No saved file, and the first check due at once.
fn scheduled() -> ScheduledTasks {
    ScheduledTasks::new(Routines::default(), None, None, Instant::now())
}

#[test]
fn a_final_reply_hides_its_block_and_schedules_it_on_the_next_poll() {
    let mut feature = scheduled();
    let mut host = FakeHost::default();
    let chat = ChatId::Task("t1".to_string());
    assert_eq!(feature.read_reply(&chat, STAPLES, true), "I'll tell you.");
    assert!(host.infos.is_empty());
    feature.poll(&mut host);
    let (said_in, line) = &host.infos[0];
    assert_eq!(said_in, &chat);
    assert!(line.starts_with("Scheduled: Staples · "), "{line}");
    assert_eq!(host.badges.last(), Some(&(PAGE_ID, 1)));
}

#[test]
fn an_interim_reply_hides_its_block_without_scheduling() {
    let mut feature = scheduled();
    let mut host = FakeHost::default();
    assert_eq!(
        feature.read_reply(&ChatId::Main, STAPLES, false),
        "I'll tell you."
    );
    feature.poll(&mut host);
    assert!(host.infos.is_empty());
    assert_eq!(feature.routines.active_count(), 0);
}

#[test]
fn a_reply_without_a_block_is_left_as_is() {
    let mut feature = scheduled();
    assert_eq!(feature.read_reply(&ChatId::Main, "hello", true), "hello");
}

#[test]
fn chat_messages_tell_claude_what_is_scheduled() {
    let mut feature = scheduled();
    let mut host = FakeHost::default();
    feature.read_reply(&ChatId::Main, STAPLES, true);
    feature.poll(&mut host);
    let instruction = feature
        .chat_instruction(1_700_000_000)
        .expect("an instruction");
    assert!(instruction.contains("Staples"), "{instruction}");
}

#[test]
fn deleting_from_the_page_removes_it_and_updates_the_badge() {
    let mut feature = scheduled();
    let mut host = FakeHost::default();
    feature.read_reply(&ChatId::Main, STAPLES, true);
    feature.poll(&mut host);
    let id = feature.routines.display()[0].id;
    feature.inbox.sender().send(Msg::Delete(id));
    feature.poll(&mut host);
    assert!(feature.routines.get(id).is_none());
    assert_eq!(host.badges.last(), Some(&(PAGE_ID, 0)));
}

#[test]
fn without_claude_a_due_watch_waits_and_nothing_runs() {
    let mut feature = scheduled();
    let mut host = FakeHost::default();
    feature.read_reply(&ChatId::Main, STAPLES, true);
    feature.poll(&mut host);
    feature.next_check = Instant::now() - Duration::from_secs(1);
    feature.poll(&mut host);
    assert!(!feature.is_busy());
    assert!(host.notes.is_empty());
}

#[test]
fn registers_the_scheduled_page() {
    let page = scheduled().pages()[0];
    assert_eq!((page.id, page.title), (PAGE_ID, "Scheduled"));
}

#[test]
fn a_schedule_request_not_yet_applied_keeps_meow_from_restarting() {
    let mut feature = scheduled();
    let mut host = FakeHost::default();
    feature.read_reply(&ChatId::Main, STAPLES, true);
    assert!(feature.is_busy(), "waiting in the mailbox");
    feature.poll(&mut host);
    assert!(!feature.is_busy());
}

#[test]
fn an_unreadable_file_is_reported_at_startup() {
    let feature = ScheduledTasks::new(
        Routines::default(),
        None,
        Some("bad json".to_string()),
        Instant::now(),
    );
    assert_eq!(
        feature.startup_error().as_deref(),
        Some("bad json. Scheduled tasks won't be saved.")
    );
}
