use meow::activity::{Banners, Kind};

use super::{NotificationSettings, Notifications};
use crate::app::feature::Feature;
use crate::features::testing::{FakeHost, change_setting, describe_settings};

fn notifications() -> Notifications {
    Notifications::from_settings(Ok(NotificationSettings::default()))
}

fn flip(feature: &mut Notifications, host: &mut FakeHost, title: &str) {
    change_setting(&feature.settings(), "Notifications", title, "");
    feature.poll(host);
}

#[test]
fn every_banner_is_on_by_default_and_told_to_the_app_at_launch() {
    let mut feature = notifications();
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    assert_eq!(host.banner_settings, [Banners::default()]);
    assert_eq!(
        describe_settings(&feature.settings()),
        [
            "# Notifications",
            "[x] toggle: Show macOS banners",
            "[x] toggle: Slack messages",
            "[x] toggle: Calendar changes",
            "[x] toggle: New tasks",
            "[x] toggle: Scheduled tasks",
            "[x] toggle: Autopilot",
            "note: News without a banner still goes to Activity. Tasks resolved at the source \
             never show one.",
        ]
    );
}

#[test]
fn muting_a_kind_tells_the_app_and_saves_it() {
    let mut feature = notifications();
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    flip(&mut feature, &mut host, "Slack messages");
    let banners = host.banner_settings.last().unwrap();
    assert!(!banners.allows(Kind::Slack));
    assert!(banners.allows(Kind::Tasks));
    assert_eq!(host.settings_changes, 1);
    let saved: NotificationSettings = host.settings.get().unwrap();
    assert_eq!(saved.banners.muted, [Kind::Slack]);
    // Turned back on, it is no longer muted.
    flip(&mut feature, &mut host, "Slack messages");
    assert!(host.banner_settings.last().unwrap().allows(Kind::Slack));
}

#[test]
fn turning_banners_off_hides_the_kinds_and_stays_off_at_next_launch() {
    let mut feature = notifications();
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    flip(&mut feature, &mut host, "Show macOS banners");
    assert!(!host.banner_settings.last().unwrap().on);
    assert_eq!(
        describe_settings(&feature.settings())[1],
        "toggle: Show macOS banners"
    );
    assert_eq!(describe_settings(&feature.settings()).len(), 3);

    let mut next = Notifications::from_settings(host.settings.get());
    let mut host = FakeHost::default();
    next.poll(&mut host);
    assert!(!host.banner_settings[0].on);
}

#[test]
fn unreadable_settings_keep_every_banner_and_say_why() {
    let feature = Notifications::from_settings(Err("broken".to_string()));
    assert_eq!(feature.startup_error().as_deref(), Some("broken"));
    assert!(feature.settings.banners.on);
}
