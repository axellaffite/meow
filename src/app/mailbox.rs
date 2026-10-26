//! A feature's own message queue. UI callbacks (menu items, buttons) only
//! send; the feature drains its mailbox in `Feature::poll`, so no callback
//! runs while app state is borrowed. Same rule as `ui::events`, without one
//! enum shared by every feature.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

type Queue<M> = Rc<RefCell<VecDeque<M>>>;

pub(crate) struct Mailbox<M> {
    queue: Queue<M>,
}

/// Sends to a mailbox; cheap to clone into as many callbacks as needed.
pub(crate) struct Sender<M> {
    queue: Queue<M>,
}

impl<M> Mailbox<M> {
    pub(crate) fn new() -> Self {
        Self {
            queue: Rc::default(),
        }
    }

    pub(crate) fn sender(&self) -> Sender<M> {
        Sender {
            queue: Rc::clone(&self.queue),
        }
    }

    /// Whether nothing was sent since the last drain.
    pub(crate) fn is_empty(&self) -> bool {
        self.queue.borrow().is_empty()
    }

    /// The messages sent since the last drain, oldest first.
    pub(crate) fn drain(&self) -> Vec<M> {
        self.queue.borrow_mut().drain(..).collect()
    }
}

impl<M> Default for Mailbox<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> Sender<M> {
    pub(crate) fn send(&self, message: M) {
        self.queue.borrow_mut().push_back(message);
    }
}

impl<M> Clone for Sender<M> {
    fn clone(&self) -> Self {
        Self {
            queue: Rc::clone(&self.queue),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Mailbox;

    #[test]
    fn drains_what_every_sender_sent_in_order() {
        let mailbox = Mailbox::new();
        let first = mailbox.sender();
        let second = first.clone();
        first.send(1);
        second.send(2);
        first.send(3);
        assert_eq!(mailbox.drain(), [1, 2, 3]);
        assert!(mailbox.drain().is_empty());
    }

    #[test]
    fn is_empty_until_something_is_sent_and_again_once_drained() {
        let mailbox = Mailbox::new();
        assert!(mailbox.is_empty());
        mailbox.sender().send(1);
        assert!(!mailbox.is_empty());
        mailbox.drain();
        assert!(mailbox.is_empty());
    }

    #[test]
    fn a_sender_outliving_a_drain_still_delivers() {
        let mailbox = Mailbox::new();
        let sender = mailbox.sender();
        sender.send("a");
        mailbox.drain();
        sender.send("b");
        assert_eq!(mailbox.drain(), ["b"]);
    }
}
