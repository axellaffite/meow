use std::path::PathBuf;
use std::time::Duration;

use meow::conversations::ChatId;
use meow::update::Manifest;

use super::{Staged, Update};
use crate::app::feature::Feature;
use crate::features::testing::{FakeHost, click, describe};

const WHY: &str = "This is a local build of Meow: it does not update itself.";

fn local_build() -> Update {
    Update::new(None, None, Duration::from_secs(3600), Some(WHY))
}

/// A release build with a newer version already unpacked at `staged`.
fn with_staged(staged: PathBuf) -> Update {
    let own: &'static str = "a".repeat(40).leak();
    let mut update = Update::new(
        Some((PathBuf::from("/nonexistent/Meow.app"), own)),
        Some(own),
        Duration::from_secs(3600),
        None,
    );
    update.staged = Some(Staged {
        manifest: Manifest {
            commit: "b".repeat(40),
            sha256: "c".repeat(64),
        },
        path: staged,
    });
    update
}

#[test]
fn menu_shows_the_version_then_check_for_updates() {
    assert_eq!(
        describe(&local_build().menu()),
        ["label: Version: local build", "action: Check for Updates"]
    );
}

#[test]
fn check_from_the_menu_on_a_local_build_says_why_in_a_banner() {
    let mut update = local_build();
    let mut host = FakeHost::default();
    click(&update.menu(), &["Check for Updates"]);
    assert!(
        host.banners.is_empty(),
        "handled on the next poll, not in the click"
    );
    update.poll(&mut host);
    assert_eq!(host.banners, [WHY]);
    assert!(host.infos.is_empty());
}

#[test]
fn update_command_answers_in_its_conversation() {
    let mut update = local_build();
    let mut host = FakeHost::default();
    let chat = ChatId::Task("t1".to_string());
    assert!(update.command("/update", &chat));
    update.poll(&mut host);
    assert_eq!(host.infos, [(chat, WHY.to_string())]);
    assert!(host.banners.is_empty());
}

#[test]
fn other_commands_are_left_to_the_chat() {
    let mut update = local_build();
    assert!(!update.command("/agent fix it", &ChatId::Main));
    assert!(!update.command("update", &ChatId::Main));
}

#[test]
fn a_staged_update_is_reported_without_checking_again() {
    let mut update = with_staged(PathBuf::from("/nonexistent/staged"));
    let mut host = FakeHost::default();
    assert!(update.command("/update", &ChatId::Main));
    update.poll(&mut host);
    let (_, text) = &host.infos[0];
    assert!(text.starts_with("Meow bbbbbbb is downloaded"), "{text}");
}

#[test]
fn a_staged_update_waits_while_the_app_is_busy() {
    let mut update = with_staged(PathBuf::from("/nonexistent/staged"));
    let mut host = FakeHost::default();
    update.poll(&mut host);
    assert!(!host.quit);
    assert!(update.staged.is_some());
}

#[test]
fn a_failed_swap_keeps_meow_running_and_drops_the_staged_build() {
    let mut update = with_staged(PathBuf::from("/nonexistent/staged"));
    let mut host = FakeHost {
        idle: true,
        ..FakeHost::default()
    };
    update.poll(&mut host);
    assert!(!host.quit);
    assert!(update.staged.is_none());
}
