//! Objective-C target for buttons, menu items and the input field.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSControl, NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags, NSTextDelegate,
    NSTextFieldDelegate, NSTextView, NSTextViewDelegate, NSWindowDelegate,
};
use objc2_foundation::{NSNotification, NSString};

use super::events::{UiEvent, push};
use super::rich_text;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowController"]
    pub struct Controller;

    impl Controller {
        #[unsafe(method(submit:))]
        fn submit(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::Submit);
        }

        #[unsafe(method(toggleChat:))]
        fn toggle_chat(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ToggleChat);
        }

        #[unsafe(method(newConversation:))]
        fn new_conversation(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::NewConversation);
        }

        #[unsafe(method(escape:))]
        fn escape(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::Escape);
        }

        /// Sent by quick-action menu items and buttons; their tag is the action index.
        #[unsafe(method(quickAction:))]
        fn quick_action(&self, sender: Option<&AnyObject>) {
            if let Some(index) = sender_tag(sender) {
                push(UiEvent::QuickAction(index));
            }
        }

        /// Theme menu items: the sender's tag is the coat index.
        #[unsafe(method(setStyle:))]
        fn set_style(&self, sender: Option<&AnyObject>) {
            if let Some(index) = sender_tag(sender) {
                push(UiEvent::SetStyle(index));
            }
        }

        #[unsafe(method(setCoat:))]
        fn set_coat(&self, sender: Option<&AnyObject>) {
            if let Some(index) = sender_tag(sender) {
                push(UiEvent::SetCoat(index));
            }
        }

        #[unsafe(method(addCoatFromPhoto:))]
        fn add_coat_from_photo(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::AddCoatFromPhoto);
        }

        #[unsafe(method(removeCoat:))]
        fn remove_coat(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::RemoveCoat);
        }

        #[unsafe(method(feed:))]
        fn feed(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::Feed);
        }

        #[unsafe(method(releaseMouse:))]
        fn release_mouse(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ReleaseMouse);
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::CheckForUpdates);
        }

        #[unsafe(method(coughUp:))]
        fn cough_up(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::CoughUp);
        }

        #[unsafe(method(showChat:))]
        fn show_chat(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ShowChat);
        }

        #[unsafe(method(showTasks:))]
        fn show_tasks(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ShowTasks);
        }

        #[unsafe(method(showSlack:))]
        fn show_slack(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ShowSlack);
        }

        #[unsafe(method(signInSlack:))]
        fn sign_in_slack(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::SignInSlack);
        }

        /// Slack conversation rows: the sender's tag is the row index.
        #[unsafe(method(showSlackDirect:))]
        fn show_slack_direct(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::SlackTab(super::slack_page::SlackTab::Direct));
        }

        #[unsafe(method(showSlackChannels:))]
        fn show_slack_channels(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::SlackTab(super::slack_page::SlackTab::Channels));
        }

        #[unsafe(method(openSlack:))]
        fn open_slack(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::OpenSlackConversation(row));
            }
        }

        #[unsafe(method(slackBack:))]
        fn slack_back(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::SlackBack);
        }

        #[unsafe(method(sendSlack:))]
        fn send_slack(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::SendSlack);
        }

        #[unsafe(method(askSlack:))]
        fn ask_slack(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::AskSlack);
        }

        #[unsafe(method(autocompleteSlack:))]
        fn autocomplete_slack(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::AutocompleteSlack);
        }

        #[unsafe(method(toggleWriteAsMe:))]
        fn toggle_write_as_me(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ToggleWriteAsMe);
        }

        #[unsafe(method(learnMyStyle:))]
        fn learn_my_style(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::LearnMyStyle);
        }

        #[unsafe(method(useSlackDraft:))]
        fn use_slack_draft(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::UseSlackDraft);
        }

        #[unsafe(method(createSlackEvent:))]
        fn create_slack_event(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::CreateSlackEvent);
        }

        #[unsafe(method(closeSlackAssist:))]
        fn close_slack_assist(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::CloseSlackAssist);
        }

        #[unsafe(method(agentSlack:))]
        fn agent_slack(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::AgentSlack);
        }

        #[unsafe(method(startSlackAgent:))]
        fn start_slack_agent(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::StartSlackAgent);
        }

        #[unsafe(method(addTask:))]
        fn add_task(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::AddTask);
        }

        /// Task rows: the sender's tag is the row index.
        #[unsafe(method(toggleTask:))]
        fn toggle_task(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::ToggleTask(row));
            }
        }

        #[unsafe(method(draftDoneReply:))]
        fn draft_done_reply(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::DraftDoneReply(row));
            }
        }

        #[unsafe(method(noDoneReply:))]
        fn no_done_reply(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::NoDoneReply(row));
            }
        }

        #[unsafe(method(toggleCompleted:))]
        fn toggle_completed(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ToggleCompleted);
        }

        #[unsafe(method(chatAboutTask:))]
        fn chat_about_task(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::ChatAboutTask(row));
            }
        }

        #[unsafe(method(openTask:))]
        fn open_task(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::OpenTask(row));
            }
        }

        #[unsafe(method(askPlan:))]
        fn ask_plan(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::AskPlan);
        }

        /// Plan page rows: the sender's tag is the item index.
        #[unsafe(method(togglePlanItem:))]
        fn toggle_plan_item(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::TogglePlanItem(row));
            }
        }

        #[unsafe(method(openPlanItem:))]
        fn open_plan_item(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::OpenPlanItem(row));
            }
        }

        #[unsafe(method(openPlanLink:))]
        fn open_plan_link(&self, sender: Option<&AnyObject>) {
            if let Some(index) = sender_tag(sender) {
                push(UiEvent::OpenPlanLink(index));
            }
        }

        #[unsafe(method(showActivity:))]
        fn show_activity(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ShowActivity);
        }

        #[unsafe(method(openActivity:))]
        fn open_activity(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::OpenActivity(row));
            }
        }

        #[unsafe(method(dismissActivity:))]
        fn dismiss_activity(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::DismissActivity(row));
            }
        }

        #[unsafe(method(clearActivity:))]
        fn clear_activity(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ClearActivity);
        }

        #[unsafe(method(showRoutines:))]
        fn show_routines(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::ShowRoutines);
        }

        #[unsafe(method(deleteRoutine:))]
        fn delete_routine(&self, sender: Option<&AnyObject>) {
            if let Some(row) = sender_tag(sender) {
                push(UiEvent::DeleteRoutine(row));
            }
        }

        #[unsafe(method(acceptPlanEvent:))]
        fn accept_plan_event(&self, sender: Option<&AnyObject>) {
            if let Some(index) = sender_tag(sender) {
                push(UiEvent::AcceptPlanEvent(index));
            }
        }

        #[unsafe(method(declinePlanEvent:))]
        fn decline_plan_event(&self, sender: Option<&AnyObject>) {
            if let Some(index) = sender_tag(sender) {
                push(UiEvent::DeclinePlanEvent(index));
            }
        }

        #[unsafe(method(runAction:))]
        fn run_action(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::RunAction);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            push(UiEvent::Quit);
        }
    }

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSControlTextEditingDelegate for Controller {
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, _view: &NSTextView, command: Sel) -> Bool {
            if command == sel!(cancelOperation:) {
                push(UiEvent::Escape);
                return Bool::YES;
            }
            // ⇧ Return adds a line break, like ⌥ Return; plain Return sends.
            if command == sel!(insertNewline:)
                && NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Shift)
            {
                // SAFETY: the field editor is a live text view; no sender is needed.
                let _: () = unsafe {
                    msg_send![_view, insertNewlineIgnoringFieldEditor: None::<&AnyObject>]
                };
                return Bool::YES;
            }
            Bool::NO
        }
    }

    unsafe impl NSTextFieldDelegate for Controller {}

    unsafe impl NSTextDelegate for Controller {}

    unsafe impl NSWindowDelegate for Controller {
        /// The bubble lost focus (another app or window was clicked).
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            push(UiEvent::FocusLost);
        }
    }

    unsafe impl NSTextViewDelegate for Controller {
        /// Clicks on a reply's "N steps" line fold or unfold it, and on a Slack
        /// message's reply count open its thread; other links open normally.
        #[unsafe(method(textView:clickedOnLink:atIndex:))]
        fn clicked_on_link(&self, _view: &NSTextView, link: &AnyObject, _index: usize) -> Bool {
            // SAFETY: links are NSURL or NSString, and both describe themselves as the URL text.
            let url: Retained<NSString> = unsafe { msg_send![link, description] };
            let url = url.to_string();
            let event = rich_text::steps_turn(&url)
                .map(UiEvent::ToggleSteps)
                .or_else(|| rich_text::thread_index(&url).map(UiEvent::OpenSlackThread));
            match event {
                Some(event) => {
                    push(event);
                    Bool::YES
                }
                None => Bool::NO,
            }
        }
    }
);

impl Controller {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's designated initializer, called once on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }
}

/// The `tag` of a menu item or button that sent an action.
fn sender_tag(sender: Option<&AnyObject>) -> Option<usize> {
    // SAFETY: these actions are only sent by NSMenuItem and NSButton, which both answer `tag`.
    let tag: isize = unsafe { msg_send![sender?, tag] };
    usize::try_from(tag).ok()
}
