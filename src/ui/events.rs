//! Queue between AppKit callbacks and the app tick.
//!
//! Callbacks only enqueue; the tick handles everything, so no callback ever
//! runs while app state is borrowed.

use std::cell::RefCell;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiEvent {
    CatClicked,
    /// The cat was picked up with the mouse.
    Grab,
    /// Dragged cat window origin, in screen coordinates.
    DragTo {
        x: f64,
        y: f64,
    },
    Drop,
    /// The mouse moved this far over the cat (petting).
    Stroke(f64),
    /// Put down a bowl of kibble for the cat.
    Feed,
    /// Let a mouse run across the screen for the cat to chase.
    ReleaseMouse,
    /// Have the cat cough up a hairball.
    CoughUp,
    /// The hairball was scrubbed with the mouse button down, this far (points).
    WipeHairball(f64),
    /// The hairball was clicked.
    TapHairball,
    /// Index into the loaded quick actions.
    QuickAction(usize),
    /// Run the quick action shown on the chat page again.
    RunAction,
    /// Fold or unfold the working steps of a reply (transcript entry index).
    ToggleSteps(usize),
    ShowChat,
    ShowTasks,
    /// Add the text typed in the task window.
    AddTask,
    /// Row index in the task window, as last rendered.
    ToggleTask(usize),
    /// A task just ticked in this row: done, and Claude drafts the reply saying so.
    DraftDoneReply(usize),
    /// A task just ticked in this row: done, nothing to answer.
    NoDoneReply(usize),
    /// Show or hide the completed tasks.
    ToggleCompleted,
    OpenTask(usize),
    /// Open the conversation about the task in this row.
    ChatAboutTask(usize),
    /// Ask about the day plan with the text typed under it.
    AskPlan,
    /// Item index on the plan page, as last rendered.
    TogglePlanItem(usize),
    OpenPlanItem(usize),
    /// Agenda link index on the plan page.
    OpenPlanLink(usize),
    /// Accept or decline the invitation of this agenda event on the plan page.
    AcceptPlanEvent(usize),
    DeclinePlanEvent(usize),
    ToggleChat,
    /// Open the Activity page.
    ShowActivity,
    /// Open what the activity item in this row is about.
    OpenActivity(usize),
    /// Dismiss the activity item in this row.
    DismissActivity(usize),
    ClearActivity,
    /// Open a feature's page (a sidebar row of `Page::Feature`, a menu item).
    ShowPage(crate::ui::sidebar::Page),
    Submit,
    Escape,
    /// The bubble stopped being the focused window.
    FocusLost,
    NewConversation,
    Quit,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<UiEvent>> = const { RefCell::new(VecDeque::new()) };
}

pub fn push(event: UiEvent) {
    QUEUE.with(|queue| queue.borrow_mut().push_back(event));
}

pub fn drain() -> Vec<UiEvent> {
    QUEUE.with(|queue| queue.borrow_mut().drain(..).collect())
}
