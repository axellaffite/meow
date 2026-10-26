//! Main menu (so ⌘C/⌘V/⌘A work in the chat) and the cat's right-click menu.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuItem,
};
use objc2_foundation::NSString;

use meow::actions::QuickAction;

use super::action::Action;
use super::controller::Controller;

/// An item a feature adds to the cat's menu (see `Feature::menu`).
pub enum MenuEntry {
    /// A greyed-out line of information.
    Label(String),
    Separator,
    /// Runs `run` when chosen; it should only send to the feature's mailbox.
    /// `checked` puts a check mark before it.
    Action {
        title: String,
        checked: bool,
        run: Box<dyn Fn()>,
    },
    Submenu {
        title: String,
        entries: Vec<MenuEntry>,
    },
}

/// Where a feature's entries go in the cat's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSection {
    /// After what the cat does (Feed, Let a Mouse In…).
    Cat,
    /// Above "Quit Meow".
    App,
}

fn entry_item(mtm: MainThreadMarker, entry: MenuEntry) -> Retained<NSMenuItem> {
    match entry {
        MenuEntry::Label(text) => label(mtm, &text),
        MenuEntry::Separator => NSMenuItem::separatorItem(mtm),
        MenuEntry::Action {
            title,
            checked,
            run,
        } => {
            let action = Action::new(mtm, run);
            let entry = item(mtm, &title, Action::selector(), "", Some(&action));
            // SAFETY: the item holds its represented object strongly, which keeps
            // the action alive as long as the item; its target alone would not.
            unsafe { entry.setRepresentedObject(Some(&action)) };
            if checked {
                entry.setState(NSControlStateValueOn);
            }
            entry
        }
        MenuEntry::Submenu { title, entries } => {
            let items: Vec<_> = entries
                .into_iter()
                .map(|entry| entry_item(mtm, entry))
                .collect();
            let entry = submenu_item(mtm, &menu(mtm, &title, &items));
            entry.setTitle(&NSString::from_str(&title));
            entry
        }
    }
}

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

/// The cat's right-click menu, empty until `fill_cat_menu`.
pub fn cat_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    menu(mtm, "Meow", &[])
}

/// Fills (or refills) the cat's menu: the quick actions, the pages, what the
/// cat does then the features' `cat` entries, and the features' `app`
/// entries above "Quit Meow". `pig` names the hairball item for Dédé.
pub fn fill_cat_menu(
    mtm: MainThreadMarker,
    cat_menu: &NSMenu,
    controller: &Controller,
    actions: &[QuickAction],
    cat: Vec<MenuEntry>,
    app: Vec<MenuEntry>,
    pig: bool,
) {
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
    // Dédé poops where the cats cough up hairballs.
    let hairball = if pig { "Poop" } else { "Hairball" };
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
        item(mtm, "Feed", sel!(feed:), "", Some(target)),
        item(mtm, "Let a Mouse In", sel!(releaseMouse:), "", Some(target)),
        item(mtm, hairball, sel!(coughUp:), "", Some(target)),
    ]);
    items.extend(cat.into_iter().map(|entry| entry_item(mtm, entry)));
    items.push(NSMenuItem::separatorItem(mtm));
    items.extend(app.into_iter().map(|entry| entry_item(mtm, entry)));
    items.push(item(mtm, "Quit Meow", sel!(quit:), "", Some(target)));
    cat_menu.removeAllItems();
    for entry in &items {
        cat_menu.addItem(entry);
    }
}
