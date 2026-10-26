use std::time::{Duration, Instant};

use meow::tasks::{self, FoundTask};

use super::BackgroundCheck;
use crate::app::feature::Feature;
use crate::features::testing::FakeHost;

const NOW: u64 = 1_760_000_000;

fn found(title: &str, source: &str) -> FoundTask {
    FoundTask {
        title: title.to_string(),
        source: Some(source.to_string()),
        url: None,
    }
}

/// A host whose task list holds `open`.
fn host_with(open: &[FoundTask]) -> FakeHost {
    let mut host = FakeHost::default();
    host.tasks.merge(open, NOW);
    host
}

/// Checks every 5 minutes, and none started yet.
fn every_five_minutes() -> BackgroundCheck {
    BackgroundCheck::new(Some(Duration::from_secs(5 * 60)))
}

fn answer(done: &[String], new: &[FoundTask]) -> String {
    let new: Vec<_> = new
        .iter()
        .map(|f| {
            format!(
                r#"{{"title": "{}", "source": "{}"}}"#,
                f.title,
                f.source.as_deref().unwrap_or("")
            )
        })
        .collect();
    format!(
        "Done.\n```meow-sync\n{{\"done\": {done:?}, \"new\": [{}]}}\n```",
        new.join(", ")
    )
}

#[test]
fn a_check_closes_resolved_tasks_adds_new_ones_and_says_so_in_activity() {
    let review = found("Review PR 42", "GitHub");
    let mut host = host_with(std::slice::from_ref(&review));
    let mut check = every_five_minutes();
    check.started_at = NOW;
    let review_id = tasks::id_for(&review).unwrap();
    let reply = found("Answer Ana", "Slack");
    check.finish(
        Ok(answer(std::slice::from_ref(&review_id), &[reply])),
        &mut host,
    );

    assert!(host.tasks.get(&review_id).unwrap().is_done());
    let open: Vec<_> = host.tasks.open().iter().map(|t| t.title.clone()).collect();
    assert_eq!(open, ["Answer Ana"]);
    assert_eq!(host.tasks_changes, 1);
    let titles: Vec<_> = host.notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(
        titles,
        ["1 new task waiting on you", "1 task resolved at the source"]
    );
    let status = host.tasks_status.last().cloned().flatten().unwrap();
    assert!(status.starts_with("checked at "), "{status}");
}

#[test]
fn tasks_already_listed_or_closed_are_not_announced_again() {
    let review = found("Review PR 42", "GitHub");
    let mut host = host_with(std::slice::from_ref(&review));
    let mut check = every_five_minutes();
    check.finish(Ok(answer(&[], &[review])), &mut host);
    assert!(host.notes.is_empty());
    assert_eq!(host.tasks_changes, 0);
}

#[test]
fn a_failed_check_shows_in_the_tasks_header_and_changes_nothing() {
    let mut host = host_with(&[found("Review PR 42", "GitHub")]);
    let mut check = every_five_minutes();
    check.finish(Err("offline".to_string()), &mut host);
    assert_eq!(
        host.tasks_status.last(),
        Some(&Some("last check failed".to_string()))
    );
    assert!(host.notes.is_empty());
    assert_eq!(host.tasks.open().len(), 1);
}

#[test]
fn an_answer_without_its_block_counts_as_a_failure() {
    let mut host = FakeHost::default();
    let mut check = every_five_minutes();
    check.finish(Ok("I looked, all good.".to_string()), &mut host);
    assert_eq!(
        host.tasks_status.last(),
        Some(&Some("last check failed".to_string()))
    );
}

#[test]
fn without_claude_a_due_check_does_not_start() {
    let mut host = FakeHost::default();
    let mut check = every_five_minutes();
    check.next_at = Instant::now() - Duration::from_secs(1);
    check.poll(&mut host);
    assert!(!check.is_busy());
}

#[test]
fn turned_off_the_tasks_header_shows_no_status() {
    let mut host = FakeHost::default();
    let mut check = BackgroundCheck::new(None);
    check.show_status(&mut host);
    assert_eq!(host.tasks_status, [None]);
}
