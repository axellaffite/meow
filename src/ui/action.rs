//! An Objective-C target that runs a Rust closure, so a feature wires its own
//! menu items and buttons: no selector in `Controller`, no variant in
//! `UiEvent`. The closure should only send to the feature's mailbox.
//!
//! AppKit holds targets weakly: whoever builds the control keeps the action
//! alive (a menu item keeps it as its represented object, see `ui::menus`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSTextDelegate, NSTextView, NSTextViewDelegate};
use objc2_foundation::NSString;

pub struct ActionIvars {
    run: Box<dyn Fn()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowAction"]
    #[ivars = ActionIvars]
    pub struct Action;

    impl Action {
        #[unsafe(method(run:))]
        fn run(&self, _sender: Option<&AnyObject>) {
            (self.ivars().run)();
        }
    }
);

impl Action {
    pub fn new(mtm: MainThreadMarker, run: impl Fn() + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ActionIvars { run: Box::new(run) });
        // SAFETY: NSObject's designated initializer, called once on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    /// The selector to pair with this target on a control.
    pub fn selector() -> Sel {
        sel!(run:)
    }
}

/// The targets of one page's controls: each runs `send` with the action it
/// was made for. The sink keeps them alive, as controls hold their target
/// weakly; `clear` lets go of those whose controls were removed, e.g. rows
/// drawn again.
pub struct ActionSink<A> {
    mtm: MainThreadMarker,
    send: Rc<dyn Fn(A)>,
    kept: RefCell<Vec<Retained<Action>>>,
}

impl<A: Clone + 'static> ActionSink<A> {
    pub fn new(mtm: MainThreadMarker, send: impl Fn(A) + 'static) -> Self {
        Self {
            mtm,
            send: Rc::new(send),
            kept: RefCell::new(Vec::new()),
        }
    }

    /// Another sink sending the same way, with targets of its own to clear.
    pub fn sibling(&self) -> Self {
        Self {
            mtm: self.mtm,
            send: Rc::clone(&self.send),
            kept: RefCell::new(Vec::new()),
        }
    }

    /// A function sending actions the same way, for a delegate of the page.
    pub fn sender(&self) -> Rc<dyn Fn(A)> {
        Rc::clone(&self.send)
    }

    /// A target sending `action`, to pair with `Action::selector()`.
    pub fn target(&self, action: A) -> Retained<Action> {
        let send = Rc::clone(&self.send);
        let target = Action::new(self.mtm, move || send(action.clone()));
        self.kept.borrow_mut().push(target.clone());
        target
    }

    pub fn clear(&self) {
        self.kept.borrow_mut().clear();
    }
}

pub struct LinkDelegateIvars {
    click: Box<dyn Fn(&str) -> bool>,
}

define_class!(
    /// A text view's delegate handing link clicks to a closure, which says
    /// whether it took the link; other links open normally. Whoever sets it
    /// keeps it alive: text views hold their delegate weakly.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowLinkDelegate"]
    #[ivars = LinkDelegateIvars]
    pub struct LinkDelegate;

    unsafe impl NSObjectProtocol for LinkDelegate {}

    unsafe impl NSTextDelegate for LinkDelegate {}

    unsafe impl NSTextViewDelegate for LinkDelegate {
        #[unsafe(method(textView:clickedOnLink:atIndex:))]
        fn clicked_on_link(&self, _view: &NSTextView, link: &AnyObject, _index: usize) -> Bool {
            // SAFETY: links are NSURL or NSString, and both describe themselves as the URL text.
            let url: Retained<NSString> = unsafe { msg_send![link, description] };
            Bool::new((self.ivars().click)(&url.to_string()))
        }
    }
);

impl LinkDelegate {
    pub fn new(mtm: MainThreadMarker, click: impl Fn(&str) -> bool + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LinkDelegateIvars {
            click: Box::new(click),
        });
        // SAFETY: NSObject's designated initializer, called once on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }
}
