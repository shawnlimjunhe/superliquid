use std::collections::VecDeque;

use super::{message::HotStuffMessage, replica::ViewNumber};

pub struct MessageWindow {
    // use a vecDeque instead of a linkedlist here for memory locality
    pub messages: VecDeque<Vec<HotStuffMessage>>,
    lowest_view: ViewNumber,
}

impl MessageWindow {
    pub fn new(view_number: ViewNumber) -> Self {
        MessageWindow {
            messages: VecDeque::new(),
            lowest_view: view_number,
        }
    }

    pub fn prune_before_view(&mut self, view: ViewNumber) {
        if view < self.lowest_view {
            // no messages to prune
            return;
        }

        if (view as usize) > (self.lowest_view as usize) + self.messages.len() {
            // prune all current messages
            self.messages = VecDeque::new();
            self.lowest_view = view;
            return;
        }

        let to_remove = view - self.lowest_view;

        for _ in 0..to_remove {
            self.messages.pop_front();
        }
        self.lowest_view = view;
    }

    pub fn push(&mut self, msg: HotStuffMessage) -> bool {
        if msg.view_number < self.lowest_view {
            return false;
        }

        let index = (msg.view_number - self.lowest_view) as usize;
        let opt_vector: Option<&mut Vec<HotStuffMessage>> = self.messages.get_mut(index as usize);
        match opt_vector {
            Some(vector) => {
                vector.push(msg);
            }
            None => {
                let vector = vec![msg];
                while index > self.messages.len() {
                    self.messages.push_back(vec![]);
                }

                self.messages.insert(index as usize, vector);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::node::state::PeerId;

    use super::*;

    fn dummy_message(view: ViewNumber, sender: PeerId) -> HotStuffMessage {
        HotStuffMessage::new(None, None, view, sender)
    }

    #[test]
    fn insert_message_at_current_view() {
        let mut window = MessageWindow::new(5);
        let msg = dummy_message(5, 1);
        let inserted = window.push(msg.clone());
        assert!(inserted);
        assert_eq!(window.messages.len(), 1);
        assert_eq!(window.messages[0], vec![msg]);
    }

    #[test]
    fn insert_message_at_future_view() {
        let mut window = MessageWindow::new(3);
        let msg = dummy_message(5, 2);
        let inserted = window.push(msg.clone());
        assert!(inserted);
        assert_eq!(window.messages.len(), 3); // index 0: view 3, index 2: view 5
        assert_eq!(window.messages[2], vec![msg]);
    }

    #[test]
    fn insert_message_at_past_view_returns_false() {
        let mut window = MessageWindow::new(4);
        let msg = dummy_message(3, 1);
        let inserted = window.push(msg);
        assert!(!inserted);
        assert!(window.messages.is_empty());
    }

    #[test]
    fn prune_before_view_removes_older_messages() {
        let mut window = MessageWindow::new(2);
        for v in 2..6 {
            window.push(dummy_message(v, 1));
        }
        window.prune_before_view(4);
        assert_eq!(window.lowest_view, 4);
        assert_eq!(window.messages.len(), 2); // views 4 and 5
        assert_eq!(window.messages[0][0].view_number, 4);
    }

    #[test]
    fn prune_beyond_current_window_clears_all() {
        let mut window = MessageWindow::new(1);
        window.push(dummy_message(1, 1));
        window.push(dummy_message(2, 2));
        window.prune_before_view(10);
        assert_eq!(window.messages.len(), 0);
        assert_eq!(window.lowest_view, 10);
    }

    #[test]
    fn insert_multiple_messages_same_view() {
        let mut window = MessageWindow::new(0);
        let msg1 = dummy_message(1, 1);
        let msg2 = dummy_message(1, 2);
        window.push(msg1.clone());
        window.push(msg2.clone());
        assert_eq!(window.messages.len(), 2); // view 0 and view 1
        assert_eq!(window.messages[1].len(), 2);
        assert!(window.messages[1].contains(&msg1));
        assert!(window.messages[1].contains(&msg2));
    }
}
