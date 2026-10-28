//! Every Objective-C class the app defines registers. objc2 builds a class
//! the first time it is used, at the app's launch, and panics there when the
//! class cannot be made: ivars aligned past 8 bytes (a `glam::Mat4` is
//! aligned to 16, so box such ivars), a method whose types do not match the
//! one it overrides (checked in debug builds), a name taken twice. Each
//! class is made here instead, so a launch never finds out first.

use std::collections::HashSet;
use std::path::Path;

use objc2::ClassType;

/// The name of every class the app defines, registering each.
fn every_class() -> Vec<&'static str> {
    [
        super::action::Action::class(),
        super::action::LinkDelegate::class(),
        super::bubble::BubblePanel::class(),
        super::bubble::BubbleView::class(),
        super::cat_window::CatView::class(),
        super::controller::Controller::class(),
        super::flipped::FlippedView::class(),
        super::link_button::LinkButton::class(),
        super::signin::NewWindowsInPlace::class(),
        super::widgets::GrowingField::class(),
        super::widgets::PaddedCell::class(),
    ]
    .into_iter()
    .map(|class| class.name().to_str().expect("class names are UTF-8"))
    .collect()
}

#[test]
fn every_class_registers_under_its_own_meow_name() {
    let names = every_class();
    let unique: HashSet<_> = names.iter().collect();
    assert_eq!(
        unique.len(),
        names.len(),
        "a name is taken twice: {names:?}"
    );
    for name in &names {
        assert!(name.starts_with("Meow"), "{name} is not one of Meow's");
    }
}

#[test]
fn every_defined_class_is_listed_above() {
    // A new `define_class!` must join `every_class`, or its launch panic
    // would go unseen again.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let defined = count_definitions(&src);
    assert_eq!(
        defined,
        every_class().len(),
        "{defined} classes are defined under src/: list the new one in every_class"
    );
}

fn count_definitions(dir: &Path) -> usize {
    let mut count = 0;
    for entry in std::fs::read_dir(dir).expect("src/ is readable") {
        let path = entry.expect("src/ is readable").path();
        if path.is_dir() {
            count += count_definitions(&path);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(&path).expect("sources are UTF-8");
            // `define_class!(`, `objc2::define_class! {`...; not a comment.
            count += text
                .lines()
                .map(str::trim_start)
                .filter(|line| !line.starts_with("//"))
                .filter(|line| line.contains(concat!("define_class", "!")))
                .count();
        }
    }
    count
}
