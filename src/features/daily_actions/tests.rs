use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use meow::actions::{Layout, QuickAction};
use meow::claude::ClaudeEnv;
use meow::schedule::DailyTime;

use super::DailyActions;
use crate::app::feature::Feature;
use crate::features::testing::FakeHost;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Never started: tests only check what the feature asks the host to run.
fn claude() -> ClaudeEnv {
    ClaudeEnv {
        binary: PathBuf::from("/nonexistent/claude"),
        path_var: String::new(),
        cwd: PathBuf::from("/nonexistent"),
        model: "haiku".to_string(),
        meow_bin: None,
    }
}

fn action(title: &str, daily_at: Option<DailyTime>) -> QuickAction {
    QuickAction {
        title: title.to_string(),
        prompt: "Do it.".to_string(),
        layout: Layout::Text,
        daily_at,
    }
}

/// Due since midnight: any time today is past it.
fn at_midnight() -> Option<DailyTime> {
    Some(DailyTime { hour: 0, minute: 0 })
}

/// A ready host with a plain action, then a daily one.
fn ready_host() -> FakeHost {
    FakeHost {
        claude_env: Some(claude()),
        actions: vec![action("Inbox", None), action("Planning", at_midnight())],
        ..FakeHost::default()
    }
}

/// With its first check due now.
fn daily() -> DailyActions {
    let mut daily = DailyActions::new();
    daily.next_check = Instant::now() - Duration::from_secs(1);
    daily
}

#[test]
fn a_daily_action_that_did_not_run_today_runs_once() {
    let mut host = ready_host();
    let mut daily = daily();
    daily.poll(&mut host);
    assert_eq!(host.ran_actions, [1]);
    daily.next_check = Instant::now() - Duration::from_secs(1);
    daily.poll(&mut host);
    assert_eq!(host.ran_actions, [1], "tried once, retried only later");
}

#[test]
fn one_that_already_ran_today_waits_for_tomorrow() {
    let mut host = ready_host();
    host.results.record("Planning", "done", None, None, now());
    let mut daily = daily();
    daily.poll(&mut host);
    assert!(host.ran_actions.is_empty());
}

#[test]
fn it_waits_while_the_chat_is_in_use() {
    let mut host = FakeHost {
        chat_in_use: true,
        ..ready_host()
    };
    let mut daily = daily();
    daily.poll(&mut host);
    assert!(host.ran_actions.is_empty());
}

#[test]
fn it_waits_for_claude() {
    let mut host = FakeHost {
        claude_env: None,
        ..ready_host()
    };
    let mut daily = daily();
    daily.poll(&mut host);
    assert!(host.ran_actions.is_empty());
}

#[test]
fn it_looks_once_a_minute_at_most() {
    let mut host = ready_host();
    let mut daily = DailyActions::new();
    daily.poll(&mut host);
    assert!(
        host.ran_actions.is_empty(),
        "the first look waits after launch"
    );
}
