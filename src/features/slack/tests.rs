use meow::activity::Link;

use super::{Slack, SlackSettings};
use crate::app::feature::Feature;
use crate::features::testing::FakeHost;
use crate::ui::sidebar::Page;
use crate::ui::{autopilot_page, slack_page};

fn slack() -> Slack {
    Slack::signed_out(Ok(SlackSettings::default()))
}

#[test]
fn slack_and_autopilot_pages_keep_their_place_in_the_sidebar() {
    let pages: Vec<_> = slack()
        .pages()
        .iter()
        .map(|page| (page.id, page.title, page.order))
        .collect();
    assert_eq!(
        pages,
        [
            (slack_page::PAGE_ID, "Slack", 30),
            (autopilot_page::PAGE_ID, "Autopilot", 50),
        ]
    );
}

#[test]
fn autopilot_news_opens_the_autopilot_page() {
    let mut feature = slack();
    let mut host = FakeHost::default();
    assert!(feature.open_link(&Link::Autopilot, &mut host));
    assert_eq!(host.opened, [Page::Feature(autopilot_page::PAGE_ID)]);
}

#[test]
fn other_links_are_left_to_the_app() {
    let mut feature = slack();
    let mut host = FakeHost::default();
    assert!(!feature.open_link(&Link::Tasks, &mut host));
    assert!(host.opened.is_empty());
}

#[test]
fn esc_away_from_the_slack_page_is_left_to_the_app() {
    let mut feature = slack();
    assert!(!feature.escape(&mut FakeHost::default()));
}

#[test]
fn unreadable_settings_are_told_at_launch() {
    let feature = Slack::signed_out(Err("Could not read settings".to_string()));
    assert_eq!(
        feature.startup_error().as_deref(),
        Some("Could not read settings. Settings changes won't be saved.")
    );
    assert_eq!(feature.settings, SlackSettings::default());
}

#[test]
fn snapshots_show_samples_of_both_pages() {
    assert_eq!(
        slack().snapshot_steps(),
        [
            "autopilot",
            "slack-list",
            "slack-conversation",
            "slack-focus",
            "slack-thread",
            "slack-assist",
        ]
    );
}

#[test]
fn nothing_runs_until_asked() {
    assert!(!slack().is_busy());
}
