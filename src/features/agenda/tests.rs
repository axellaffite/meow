use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use meow::actions::{Layout, QuickAction};
use meow::plan::{DayPlan, Event};

use super::AgendaRefresh;
use crate::app::feature::Feature;
use crate::features::testing::FakeHost;

const PLANNING: &str = "Today's planning";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn plan_action() -> QuickAction {
    QuickAction {
        title: PLANNING.to_string(),
        prompt: "Plan my day.".to_string(),
        layout: Layout::Plan,
        daily_at: None,
    }
}

/// An all-day event, never past; not announced when it comes or goes.
fn all_day(title: &str) -> Event {
    serde_json::from_value(serde_json::json!({ "title": title })).unwrap()
}

/// An invitation waiting for an answer, announced when it shows up.
fn invitation(title: &str) -> Event {
    serde_json::from_value(serde_json::json!({
        "title": title,
        "id": format!("event-{title}"),
        "response": "needsAction",
    }))
    .unwrap()
}

/// A host with Today's planning, whose plan made `made_at` has `agenda`.
fn host_with_plan(made_at: u64, agenda: Vec<Event>) -> FakeHost {
    let mut host = FakeHost {
        actions: vec![plan_action()],
        ..FakeHost::default()
    };
    let plan = DayPlan {
        agenda,
        ..DayPlan::default()
    };
    host.results.record(PLANNING, "", None, Some(plan), made_at);
    host
}

fn every_quarter() -> AgendaRefresh {
    AgendaRefresh::new(Some(Duration::from_secs(15 * 60)))
}

#[test]
fn a_refresh_replaces_todays_agenda_and_says_what_changed() {
    let mut host = host_with_plan(now(), vec![all_day("Standup")]);
    let mut refresh = every_quarter();
    refresh.finish(
        Ok(vec![all_day("Standup"), invitation("Offsite")]),
        &mut host,
    );
    let agenda = &host
        .results
        .get(PLANNING)
        .unwrap()
        .plan
        .as_ref()
        .unwrap()
        .agenda;
    assert_eq!(agenda.len(), 2);
    assert_eq!(host.plans_changes, 1);
    let titles: Vec<_> = host.notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, ["Invitation: Offsite"]);
}

#[test]
fn the_same_agenda_announces_nothing() {
    let mut host = host_with_plan(now(), vec![invitation("Offsite")]);
    let mut refresh = every_quarter();
    refresh.finish(Ok(vec![invitation("Offsite")]), &mut host);
    assert!(host.notes.is_empty());
}

#[test]
fn a_plan_from_another_day_is_left_alone() {
    let mut host = host_with_plan(now() - 3 * 24 * 60 * 60, vec![]);
    let mut refresh = every_quarter();
    refresh.finish(Ok(vec![all_day("Offsite")]), &mut host);
    assert_eq!(host.plans_changes, 0);
    assert!(host.notes.is_empty());
}

#[test]
fn a_failed_refresh_changes_nothing() {
    let mut host = host_with_plan(now(), vec![all_day("Standup")]);
    let mut refresh = every_quarter();
    refresh.finish(Err("offline".to_string()), &mut host);
    assert_eq!(host.plans_changes, 0);
}

#[test]
fn without_a_calendar_connector_no_refresh_starts() {
    let mut host = host_with_plan(now() - 60 * 60, vec![]);
    let mut refresh = every_quarter();
    refresh.next_at = Instant::now() - Duration::from_secs(1);
    refresh.poll(&mut host);
    assert!(!refresh.is_busy());
}

#[test]
fn turned_off_it_never_starts() {
    let mut host = host_with_plan(now() - 60 * 60, vec![]);
    let mut refresh = AgendaRefresh::new(None);
    refresh.next_at = Instant::now() - Duration::from_secs(1);
    refresh.poll(&mut host);
    assert!(!refresh.is_busy());
}
