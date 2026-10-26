use std::path::PathBuf;

use meow::appearance::{Appearance, WindowSize};
use meow::cat_photo::CustomCoat;
use meow::conversations::ChatId;
use meow::sprites::{Coat, Palette};

use super::Theme;
use crate::app::feature::Feature;
use crate::features::testing::{FakeHost, click, describe};
use crate::ui::menus::MenuSection;

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
}

#[test]
fn the_theme_menu_lists_coats_photo_themes_and_window_sizes() {
    let feature = theme(Appearance::default().with_custom(mochi()));
    assert_eq!(feature.menu_section(), MenuSection::Cat);
    assert_eq!(
        describe(&feature.menu()),
        [
            "Theme:",
            "  action: Ginger",
            "  action: Willow",
            "  action: Alfie",
            "  action: Dédé",
            "  action: Corgi",
            "  ---",
            "  [x] action: Mochi",
            "  ---",
            "  action: Add from Cat Photo…",
            "  action: Remove \u{201c}Mochi\u{201d}",
            "  ---",
            "  label: Window Size",
            "  [x] action: Standard",
            "  action: Large",
            "  action: Extra Large",
        ]
    );
}

#[test]
fn picking_a_coat_wears_and_saves_it() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    click(&feature.menu(), &["Theme", "Willow"]);
    feature.poll(&mut host);
    assert_eq!(host.worn.last(), Some(&Coat::Willow));
    assert_eq!(
        host.settings.get::<Appearance>().unwrap().coat,
        Coat::Willow
    );
    assert_eq!(host.menu_changes, 1);
    assert!(describe(&feature.menu()).contains(&"  [x] action: Willow".to_string()));
}

#[test]
fn picking_a_window_size_resizes_the_bubble_and_saves_it() {
    let mut feature = theme(Appearance::default());
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    click(&feature.menu(), &["Theme", "Extra Large"]);
    feature.poll(&mut host);
    assert_eq!(host.sizes.last(), Some(&WindowSize::Huge.points()));
    assert_eq!(
        host.settings.get::<Appearance>().unwrap().window_size,
        WindowSize::Huge
    );
}

#[test]
fn removing_the_worn_photo_theme_goes_back_to_ginger() {
    let mut feature = theme(Appearance::default().with_custom(mochi()));
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    click(&feature.menu(), &["Theme", "Remove \u{201c}Mochi\u{201d}"]);
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
    click(&feature.menu(), &["Theme", "Add from Cat Photo…"]);
    feature.poll(&mut host);
    assert!(!feature.thinking());
    assert!(host.errors.is_empty());
}

#[test]
fn a_photo_before_claude_is_ready_says_so_with_a_bang_over_the_cat() {
    let mut feature = Theme::new(Ok(Appearance::default()), a_photo);
    let mut host = FakeHost::default();
    feature.poll(&mut host);
    click(&feature.menu(), &["Theme", "Add from Cat Photo…"]);
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
    click(&feature.menu(), &["Theme", "Standard"]);
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
