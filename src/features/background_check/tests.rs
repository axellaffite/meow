use std::time::{Duration, Instant};

use meow::tasks::{self, FoundTask};

use super::BackgroundCheck;
use crate::app::feature::Feature;
use crate::features::testing::{FakeHost, change_setting, describe_settings};

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
    assert_eq!(titles, ["1 new task waiting on you"]);
    let quiet: Vec<_> = host.quiet_notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(
        quiet,
        ["1 task resolved at the source"],
        "no banner for good news"
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

fn slack(title: &str, url: &str) -> FoundTask {
    FoundTask {
        title: title.to_string(),
        source: Some("Slack".to_string()),
        url: Some(url.to_string()),
    }
}

/// A Slack link to a message posted at `at` in DM `channel`.
fn dm_link(channel: &str, at: u64) -> String {
    format!("https://x.slack.com/archives/{channel}/p{at}000100")
}

fn shown_titles(host: &FakeHost) -> Vec<String> {
    host.tasks
        .display()
        .iter()
        .filter(|t| !t.is_done())
        .map(|t| t.title.clone())
        .collect()
}

/// A check that has learned the list as it is, as after its first poll.
fn sorting_check(host: &mut FakeHost) -> BackgroundCheck {
    let mut check = every_five_minutes();
    check.triage.sort(&mut host.tasks, NOW);
    check
}

#[test]
fn a_message_to_answer_waits_out_its_grace_period_then_shows_with_a_banner() {
    let mut host = FakeHost::default();
    let mut check = sorting_check(&mut host);
    let now = super::now_secs();
    host.tasks
        .merge(&[slack("Answer Ana", &dm_link("D1", now))], now);
    check.poll(&mut host);
    assert!(shown_titles(&host).is_empty(), "held for 30 minutes");
    assert_eq!(host.tasks.held().len(), 1);
    assert!(host.notes.is_empty(), "no news while it is held");
    assert!(
        host.tasks_status
            .last()
            .unwrap()
            .as_deref()
            .unwrap()
            .ends_with("1 on hold")
    );

    // The grace period is over: it shows, with its news.
    let id = host.tasks.held()[0].id.clone();
    host.tasks.hold(&id, now - 1);
    check.poll(&mut host);
    assert_eq!(shown_titles(&host), ["Answer Ana"]);
    assert_eq!(host.notes[0].title, "1 new task waiting on you");
}

#[test]
fn the_grace_period_counts_from_when_the_message_was_posted() {
    let mut host = FakeHost::default();
    let mut check = sorting_check(&mut host);
    let now = super::now_secs();
    // Posted an hour ago, found only now: nothing left to wait.
    host.tasks
        .merge(&[slack("Answer Bo", &dm_link("D2", now - 3600))], now);
    check.poll(&mut host);
    assert_eq!(shown_titles(&host), ["Answer Bo"]);
}

#[test]
fn only_messages_wait_and_tasks_listed_at_launch_stay_shown() {
    let mut host = host_with(&[slack("Answer Cy", &dm_link("D3", super::now_secs()))]);
    let mut check = every_five_minutes();
    check.poll(&mut host);
    assert_eq!(
        shown_titles(&host),
        ["Answer Cy"],
        "already shown at launch"
    );
    host.tasks
        .merge(&[found("Fix ENG-52", "Linear")], super::now_secs());
    check.poll(&mut host);
    assert_eq!(shown_titles(&host), ["Answer Cy", "Fix ENG-52"]);
}

#[test]
fn a_held_message_answered_in_time_goes_without_a_word() {
    let mut host = FakeHost::default();
    let mut check = sorting_check(&mut host);
    let now = super::now_secs();
    let reply = slack("Answer Di", &dm_link("D4", now));
    host.tasks.merge(std::slice::from_ref(&reply), now);
    check.poll(&mut host);
    check.started_at = now;
    check.finish(
        Ok(answer(&[tasks::id_for(&reply).unwrap()], &[])),
        &mut host,
    );
    assert!(host.tasks.held().is_empty());
    assert!(shown_titles(&host).is_empty());
    assert!(host.notes.is_empty() && host.quiet_notes.is_empty());
}

#[test]
fn resolved_tasks_go_to_activity_without_a_banner() {
    let review = found("Review PR 42", "GitHub");
    let mut host = host_with(std::slice::from_ref(&review));
    let mut check = sorting_check(&mut host);
    check.started_at = NOW;
    check.finish(
        Ok(answer(&[tasks::id_for(&review).unwrap()], &[])),
        &mut host,
    );
    assert!(host.notes.is_empty());
    assert_eq!(host.quiet_notes[0].title, "1 task resolved at the source");
}

#[test]
fn a_repeat_of_a_listed_request_is_merged_into_it() {
    let mut host = host_with(&[FoundTask {
        title: "Relire la PR infra#707 de Bo".to_string(),
        source: Some("GitHub".to_string()),
        url: Some("https://github.com/acme/infra/pull/707".to_string()),
    }]);
    let mut check = sorting_check(&mut host);
    host.tasks
        .merge(&[found("Review PR infra#707 in Linear", "Linear")], NOW);
    check.poll(&mut host);
    assert_eq!(shown_titles(&host), ["Relire la PR infra#707 de Bo"]);
    assert_eq!(host.tasks.open().len(), 1);
}

#[test]
fn repeats_stay_apart_when_merging_is_turned_off() {
    let mut host = host_with(&[found("Finir ENG-60", "Linear")]);
    let mut check = sorting_check(&mut host);
    change_setting(
        &check.settings(),
        "Tasks",
        "Merge repeats of the same request",
        "",
    );
    check.poll(&mut host);
    host.tasks.merge(&[found("Mesurer ENG-60", "Linear")], NOW);
    check.poll(&mut host);
    assert_eq!(shown_titles(&host).len(), 2);
    let saved: super::TriageSettings = host.settings.get().unwrap();
    assert!(!saved.merge_repeat_tasks);
}

#[test]
fn turning_the_grace_period_off_shows_what_was_held() {
    let mut host = FakeHost::default();
    let mut check = sorting_check(&mut host);
    let now = super::now_secs();
    host.tasks
        .merge(&[slack("Answer Ed", &dm_link("D5", now))], now);
    check.poll(&mut host);
    assert!(shown_titles(&host).is_empty());
    assert_eq!(
        describe_settings(&check.settings())[1],
        "choice: Grace period for messages to answer = Off | 10 min | 20 min | [30 min] | 1 h"
    );
    change_setting(
        &check.settings(),
        "Tasks",
        "Grace period for messages to answer",
        "Off",
    );
    check.poll(&mut host);
    assert_eq!(shown_titles(&host), ["Answer Ed"]);
    let saved: super::TriageSettings = host.settings.get().unwrap();
    assert_eq!(saved.task_grace_minutes, 0);
}
