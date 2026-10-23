//! Main menu (so ⌘C/⌘V/⌘A work in the chat) and the cat's right-click menu.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenu,
    NSMenuItem,
};
use objc2_foundation::NSString;

use meow::actions::QuickAction;
use meow::settings::Settings;
use meow::sprites::Coat;
use meow::style::Style;

use super::controller::Controller;

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    key: &str,
    target: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    // SAFETY: plain initializer; `action` is a selector understood by the target or responder chain.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    // SAFETY: targets are app-lifetime objects (or nil for the responder chain).
    unsafe { item.setTarget(target) };
    item
}

/// A greyed-out line of information.
fn label(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
    // SAFETY: plain initializer; with no action the item shows disabled.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            None,
            &NSString::from_str(""),
        )
    };
    item.setEnabled(false);
    item
}

fn menu(mtm: MainThreadMarker, title: &str, items: &[Retained<NSMenuItem>]) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for entry in items {
        menu.addItem(entry);
    }
    menu
}

fn submenu_item(mtm: MainThreadMarker, submenu: &NSMenu) -> Retained<NSMenuItem> {
    let entry = NSMenuItem::new(mtm);
    entry.setSubmenu(Some(submenu));
    entry
}

/// Esc as a menu key equivalent works whatever has focus in the chat (input or transcript).
fn escape_item(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSMenuItem> {
    let entry = item(mtm, "Cancel / Close", sel!(escape:), "\u{1b}", Some(target));
    entry.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
    entry
}

pub fn install_main_menu(mtm: MainThreadMarker, controller: &Controller) {
    let target: &AnyObject = controller;
    let app_menu = menu(
        mtm,
        "Meow",
        &[item(mtm, "Quit Meow", sel!(quit:), "q", Some(target))],
    );
    let edit_menu = menu(
        mtm,
        "Edit",
        &[
            item(mtm, "Undo", sel!(undo:), "z", None),
            item(mtm, "Cut", sel!(cut:), "x", None),
            item(mtm, "Copy", sel!(copy:), "c", None),
            item(mtm, "Paste", sel!(paste:), "v", None),
            item(mtm, "Select All", sel!(selectAll:), "a", None),
            item(mtm, "Close", sel!(toggleChat:), "w", Some(target)),
            escape_item(mtm, target),
            item(mtm, "Tasks", sel!(showTasks:), "t", Some(target)),
            item(mtm, "Slack", sel!(showSlack:), "l", Some(target)),
            item(
                mtm,
                "New Conversation",
                sel!(newConversation:),
                "n",
                Some(target),
            ),
        ],
    );
    let bar = NSMenu::new(mtm);
    bar.addItem(&submenu_item(mtm, &app_menu));
    bar.addItem(&submenu_item(mtm, &edit_menu));
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&bar));
}

/// Tags the cat menu's hairball item, retitled for Dédé.
const HAIRBALL_TAG: isize = -1;

/// Fills the theme menu: one item per theme, the worn one checked, the
/// built-in ones apart from the user's own, then adding one from a photo and
/// removing the worn one when it is the user's.
pub fn fill_coat_menu(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    target: &AnyObject,
    settings: &Settings,
    adding: bool,
) {
    menu.removeAllItems();
    // Dédé poops where the cats cough up hairballs.
    let hairball = if settings.coat.is_pig() {
        "Poop"
    } else {
        "Hairball"
    };
    // SAFETY: plain getter; the cat's menu lives as long as the app.
    let parent = unsafe { menu.supermenu() };
    let sibling = parent
        .as_deref()
        .and_then(|parent| parent.itemWithTag(HAIRBALL_TAG));
    if let Some(entry) = sibling {
        entry.setTitle(&NSString::from_str(hairball));
    }
    let built_in = Coat::ALL.len();
    for (index, (name, coat)) in settings.coats().into_iter().enumerate() {
        if index == built_in {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        let entry = item(mtm, &name, sel!(setCoat:), "", Some(target));
        entry.setTag(index as isize);
        let state = if coat == settings.coat {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        };
        entry.setState(state);
        menu.addItem(&entry);
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    if adding {
        menu.addItem(&label(mtm, "Looking at Your Cat…"));
    } else {
        menu.addItem(&item(
            mtm,
            "Add from Cat Photo…",
            sel!(addCoatFromPhoto:),
            "",
            Some(target),
        ));
    }
    if let Some(custom) = settings.current_custom() {
        let title = format!("Remove \u{201c}{}\u{201d}", custom.name);
        menu.addItem(&item(mtm, &title, sel!(removeCoat:), "", Some(target)));
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&label(mtm, "Style (relaunches Meow)"));
    for (index, style) in Style::ALL.into_iter().enumerate() {
        let entry = item(mtm, style.name(), sel!(setStyle:), "", Some(target));
        entry.setTag(index as isize);
        let state = if style == settings.style {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        };
        entry.setState(state);
        menu.addItem(&entry);
    }
}

/// The cat's right-click menu, and its theme submenu to check the coat in later.
pub fn cat_menu(
    mtm: MainThreadMarker,
    controller: &Controller,
    actions: &[QuickAction],
    settings: &Settings,
    version: &str,
) -> (Retained<NSMenu>, Retained<NSMenu>) {
    let target: &AnyObject = controller;
    let mut items: Vec<_> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let entry = item(mtm, &action.title, sel!(quickAction:), "", Some(target));
            entry.setTag(index as isize);
            entry
        })
        .collect();
    if !items.is_empty() {
        items.push(NSMenuItem::separatorItem(mtm));
    }
    items.extend([
        item(mtm, "Open Chat", sel!(toggleChat:), "", Some(target)),
        item(mtm, "Tasks", sel!(showTasks:), "", Some(target)),
        item(mtm, "Slack", sel!(showSlack:), "", Some(target)),
        item(
            mtm,
            "New Conversation",
            sel!(newConversation:),
            "",
            Some(target),
        ),
        NSMenuItem::separatorItem(mtm),
    ]);
    items.extend([
        item(mtm, "Feed", sel!(feed:), "", Some(target)),
        item(mtm, "Let a Mouse In", sel!(releaseMouse:), "", Some(target)),
    ]);
    let hairball = item(mtm, "", sel!(coughUp:), "", Some(target));
    hairball.setTag(HAIRBALL_TAG);
    items.push(hairball);
    let coats = menu(mtm, "Theme", &[]);
    let theme = submenu_item(mtm, &coats);
    theme.setTitle(&NSString::from_str("Theme"));
    items.extend([
        theme,
        NSMenuItem::separatorItem(mtm),
        label(mtm, version),
        item(
            mtm,
            "Check for Updates",
            sel!(checkForUpdates:),
            "",
            Some(target),
        ),
        item(mtm, "Quit Meow", sel!(quit:), "", Some(target)),
    ]);
    let cat_menu = menu(mtm, "Meow", &items);
    // Once in the cat's menu, so it can title the hairball item too.
    fill_coat_menu(mtm, &coats, target, settings, false);
    (cat_menu, coats)
}
