use std::path::PathBuf;

use meow::appearance::{Appearance, WindowSize};
use meow::cat_photo::CustomCoat;
use meow::conversations::ChatId;
use meow::sprites::{Coat, Palette};
use meow::style::Style;

use super::Theme;
use crate::app::feature::Feature;
use crate::features::testing::{FakeHost, change_setting, describe_settings};

fn no_photo() -> Option<PathBuf> {
    None
}

fn a_photo() -> Option<PathBuf> {
    Some(PathBuf::from("/nonexistent/Mochi.jpg"))
}

fn theme(saved: Appearance) -> Theme {
    Theme::new(Ok(saved), no_photo)
}

fn mochi() -> CustomCoat {
    let fur = [20, 20, 20];
    CustomCoat {
        name: "Mochi".to_string(),
        palette: Palette {
            fur,
            stripe: fur,
            blaze: [255, 255, 255],
            muzzle: [255, 255, 255],
            legs: [255, 255, 255],
            tail: fur,
            near_eye: [60, 140, 60],
            far_eye: [60, 140, 60],
        },
    }
}

#[test]
fn the_first_poll_dresses_the_cat_and_sizes_the_bubble_as_saved() {
    let mut feature = theme(Appearance {
        coat: Coat::Willow,
        window_size: WindowSize::Large,
        ..Appearance::default()
    });
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    feature.poll(&mut host);
    assert_eq!(host.worn, [Coat::Willow]);
    assert_eq!(host.sizes, [WindowSize::Large.points()]);
    assert_eq!(host.news_shown, [true]);
}

#[test]
fn the_appearance_settings_list_coats_photo_themes_window_sizes_and_styles() {
    let feature = theme(Appearance::default().with_custom(mochi()));
    assert!(
        feature.menu().is_empty(),
        "the cat's menu opens Settings instead"
    );
    assert_eq!(
        describe_settings(&feature.settings()),
        [
            "# Appearance",
            "choice: Theme = Ginger | Willow | Alfie | Dédé | Corgi | [Mochi]",
            "button: Add from Cat Photo…",
            "button: Remove \u{201c}Mochi\u{201d}",
            "choice: Window size = [Standard] | Large | Extra Large",
            "choice: Style = [Pixel Art] | Classic",
            "note: Changing the style relaunches Meow.",
            "[x] toggle: Show news over the cat",
        ]
    );
}

/// Uses entry `title` of the Appearance settings, with `value` for a choice.
fn change(feature: &Theme, title: &str, value: &str) {
    change_setting(&feature.settings(), "Appearance", title, value);
}

#[test]
fn picking_a_coat_wears_and_saves_it() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Theme", "Willow");
    feature.poll(&mut host);
    assert_eq!(host.worn.last(), Some(&Coat::Willow));
    assert_eq!(
        host.settings.get::<Appearance>().unwrap().coat,
        Coat::Willow
    );
    assert_eq!(host.settings_changes, 1);
    assert_eq!(
        describe_settings(&feature.settings())[1],
        "choice: Theme = Ginger | [Willow] | Alfie | Dédé | Corgi"
    );
}

#[test]
fn picking_a_window_size_resizes_the_bubble_and_saves_it() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Window size", "Extra Large");
    feature.poll(&mut host);
    assert_eq!(host.sizes.last(), Some(&WindowSize::Huge.points()));
    assert_eq!(
        host.settings.get::<Appearance>().unwrap().window_size,
        WindowSize::Huge
    );
}

#[test]
fn turning_the_news_bubble_off_hides_it_and_saves_it() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Show news over the cat", "");
    feature.poll(&mut host);
    assert_eq!(host.news_shown, [true, false]);
    assert!(host.settings.get::<Appearance>().unwrap().hide_news_bubble);
    assert_eq!(
        describe_settings(&feature.settings()).last().unwrap(),
        "toggle: Show news over the cat"
    );
    // Saved off, it stays off at the next launch.
    let mut host = FakeHost::default();
    theme(Appearance {
        hide_news_bubble: true,
        ..Appearance::default()
    })
    .poll(&mut host);
    assert_eq!(host.news_shown, [false]);
}

#[test]
fn picking_a_style_saves_it_and_asks_for_a_restart_outside_the_app() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Style", "Classic");
    feature.poll(&mut host);
    assert_eq!(
        host.settings.get::<Appearance>().unwrap().style,
        Style::Classic
    );
    // A test binary is not in a bundle, so there is nothing to relaunch.
    assert_eq!(
        host.infos.last().map(|(_, text)| text.as_str()),
        Some("Restart Meow to see the new style.")
    );
    assert!(!host.quit);
}

#[test]
fn removing_the_worn_photo_theme_goes_back_to_ginger() {
    let mut feature = theme(Appearance::default().with_custom(mochi()));
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Remove \u{201c}Mochi\u{201d}", "");
    feature.poll(&mut host);
    assert_eq!(host.worn.last(), Some(&Coat::Ginger));
    assert!(
        host.settings
            .get::<Appearance>()
            .unwrap()
            .custom_coats
            .is_empty()
    );
}

#[test]
fn cancelling_the_photo_picker_does_nothing() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Add from Cat Photo…", "");
    feature.poll(&mut host);
    assert!(!feature.thinking());
    assert!(host.errors.is_empty());
}

#[test]
fn a_photo_before_claude_is_ready_says_so_with_a_bang_over_the_cat() {
    let mut feature = Theme::new(Ok(Appearance::default()), a_photo);
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Add from Cat Photo…", "");
    feature.poll(&mut host);
    assert_eq!(
        host.errors,
        [(
            ChatId::Main,
            "Could not make a theme from your photo: Claude Code is not ready yet. \
             Try again in a moment."
                .to_string()
        )]
    );
    assert_eq!(host.unread, [ChatId::Main]);
    assert!(!feature.thinking());
}

#[test]
fn picking_the_size_already_worn_resizes_the_bubble_again() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    change(&feature, "Window size", "Standard");
    feature.poll(&mut host);
    assert_eq!(
        host.sizes,
        [WindowSize::Standard.points(), WindowSize::Standard.points()]
    );
}

#[test]
fn unreadable_saved_settings_are_reported_and_defaults_worn() {
    let mut feature = Theme::new(Err("Could not read settings".to_string()), no_photo);
    let mut host = FakeHost::default();
    assert_eq!(
        feature.startup_error().as_deref(),
        Some("Could not read settings. Settings changes won't be saved.")
    );
    feature.poll(&mut host);
    assert_eq!(host.worn, [Coat::Ginger]);
}
