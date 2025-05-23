use super::{message::HotStuffMessage, replica::ViewNumber};
/// MessageWindow maintains recent HotStuff messages in a sliding window indexed by view number.
///
/// # Design Goals
/// - Efficiently store and retrieve messages grouped by view.
/// - Support fast pruning of outdated views as consensus advances.
/// - Maximize memory locality and cache efficiency.
/// - Handle gaps between views without requiring strict continuity.
///
/// # Data Structures
/// - `Vec<Vec<HotStuffMessage>>`:
///   - Outer `Vec` holds messages for consecutive views, starting from `lowest_view`.
///     - chosen over vecdeque since most of the time views is small < 5.
///   - Inner `Vec<HotStuffMessage>` stores all messages for a specific view.
///   - Chosen over `LinkedList` for better memory locality.
/// - `lowest_view: ViewNumber`:
///   - Maps logical view numbers to VecDeque indices in constant time.
///   - Advances when old views are pruned.
pub struct MessageWindow {
    // use a vecDeque instead of a linkedlist here for memory locality
    pub messages: Vec<Vec<HotStuffMessage>>,
    lowest_view: ViewNumber,
}

impl MessageWindow {
    pub fn new(view_number: ViewNumber) -> Self {
        MessageWindow {
            messages: Vec::with_capacity(4), // optimistic 3 phase
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
            self.messages = Vec::new();
            self.lowest_view = view;
            return;
        }

        let to_remove = view - self.lowest_view;

        self.messages.drain(0..to_remove as usize);

        self.lowest_view = view;
    }

    pub fn push(&mut self, msg: HotStuffMessage) -> bool {
        if msg.get_view_number() < self.lowest_view {
            return false;
        }

        let index = (msg.get_view_number() - self.lowest_view) as usize;
        let opt_vector: Option<&mut Vec<HotStuffMessage>> = self.messages.get_mut(index as usize);
        match opt_vector {
            Some(vector) => {
                vector.push(msg);
            }
            None => {
                let vector = vec![msg];
                while index > self.messages.len() {
                    self.messages.push(vec![]);
                }

                self.messages.insert(index as usize, vector);
            }
        }
        true
    }

    pub fn get_messages_for_view(&self, view: ViewNumber) -> Option<&Vec<HotStuffMessage>> {
        if view < self.lowest_view {
            return None;
        }

        let highest_view = self.lowest_view as usize + self.messages.len();
        if (view as usize) > highest_view {
            return None;
        }

        let index = (view - self.lowest_view) as usize;
        return self.messages.get(index);
    }

    pub fn iter(&self) -> MessageWindowIter<'_> {
        let mut outer = self.messages.iter();
        let inner = outer.next().map(|v| v.iter());

        MessageWindowIter { outer, inner }
    }
}

pub struct MessageWindowIter<'a> {
    outer: std::slice::Iter<'a, Vec<HotStuffMessage>>,
    inner: Option<std::slice::Iter<'a, HotStuffMessage>>,
}

impl<'a> Iterator for MessageWindowIter<'a> {
    type Item = &'a HotStuffMessage;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(inner) = &mut self.inner {
                if let Some(msg) = inner.next() {
                    return Some(msg);
                }
            }

            self.inner = self.outer.next().map(|v| v.iter());
            if self.inner.is_none() {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{hotstuff::crypto::QuorumCertificate, node::state::PeerId};

    use super::*;

    fn dummy_qc() -> QuorumCertificate {
        QuorumCertificate::mock(0)
    }

    fn dummy_message(view: ViewNumber, sender: PeerId) -> HotStuffMessage {
        HotStuffMessage::create_new_view(dummy_qc(), view, sender, 0)
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
        assert_eq!(window.messages[0][0].get_view_number(), 4);
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
