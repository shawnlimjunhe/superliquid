use std::collections::HashMap;

use super::{
    crypto::QuorumCertificate,
    message::{HotStuffMessage, HotStuffMessageType},
    replica::ViewNumber,
};

pub fn get_highest_qc_from_votes<'a>(
    curr_view: ViewNumber,
    votes: &'a Vec<HotStuffMessage>,
) -> Option<&'a QuorumCertificate> {
    println!("votes: {:?}", votes);
    votes
        .iter()
        .filter_map(|msg| match msg.message_type {
            HotStuffMessageType::NewView => {
                if msg.view_number == curr_view - 1 {
                    return msg.justify.as_ref();
                }
                None
            }
            _ => None,
        })
        .max_by_key(|qc| qc.view_number)
}

pub(crate) fn has_quorum_for_view(
    messages: &HashMap<ViewNumber, Vec<HotStuffMessage>>,
    view: ViewNumber,
    quorum_threhold: usize,
    message_type: HotStuffMessageType,
) -> bool {
    let Some(msgs) = messages.get(&view) else {
        return false;
    };

    return msgs
        .iter()
        .filter(|m| m.message_type == message_type && m.view_number == view)
        .count()
        >= quorum_threhold;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::hotstuff::{
        crypto::QuorumCertificate,
        message::{HotStuffMessage, HotStuffMessageType},
        replica::ViewNumber,
        utils::{self, has_quorum_for_view},
    };

    fn mock_qc(view_number: ViewNumber) -> QuorumCertificate {
        QuorumCertificate::mock(view_number, HotStuffMessageType::NewView)
    }

    fn new_view_msg(view: ViewNumber, qc_view: ViewNumber) -> HotStuffMessage {
        HotStuffMessage {
            message_type: HotStuffMessageType::NewView,
            view_number: view,
            node: None,
            justify: Some(mock_qc(qc_view)),
            partial_sig: None,
        }
    }

    #[test]
    fn returns_none_if_no_votes() {
        let votes = vec![];
        let result = utils::get_highest_qc_from_votes(5, &votes);
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_if_no_previous_view_matches() {
        let votes = vec![
            new_view_msg(1, 0), // curr_view - 1 = 4
            new_view_msg(2, 1),
        ];
        let result = utils::get_highest_qc_from_votes(5, &votes);
        assert!(result.is_none());
    }

    #[test]
    fn returns_qc_with_highest_view_among_matches() {
        let votes = vec![
            new_view_msg(4, 8), // 4 == curr_view - 1 (5 - 1)
            new_view_msg(4, 10),
            new_view_msg(4, 7),
        ];
        let result = utils::get_highest_qc_from_votes(5, &votes);
        assert_eq!(result.unwrap().view_number, 10);
    }

    #[test]
    fn ignores_messages_with_non_newview_type() {
        let mut votes = vec![new_view_msg(4, 5), new_view_msg(4, 6)];

        votes.push(HotStuffMessage {
            message_type: HotStuffMessageType::Prepare,
            view_number: 4,
            node: None,
            justify: Some(mock_qc(100)), // Should be ignored
            partial_sig: None,
        });

        let result = utils::get_highest_qc_from_votes(5, &votes);
        assert_eq!(result.unwrap().view_number, 6); // not 100
    }

    fn make_msg(view: ViewNumber, msg_type: HotStuffMessageType) -> HotStuffMessage {
        HotStuffMessage {
            view_number: view,
            message_type: msg_type,
            node: None,
            justify: None,
            partial_sig: None,
        }
    }

    #[test]
    fn test_returns_false_if_view_absent() {
        let messages = HashMap::new();
        let result = has_quorum_for_view(&messages, 1, 2, HotStuffMessageType::NewView);
        assert!(!result);
    }

    #[test]
    fn test_returns_false_if_not_enough_messages_of_type() {
        let mut messages = HashMap::new();
        messages.insert(
            2,
            vec![
                make_msg(2, HotStuffMessageType::NewView),
                make_msg(2, HotStuffMessageType::Prepare),
            ],
        );
        let result = has_quorum_for_view(&messages, 2, 2, HotStuffMessageType::NewView);
        assert!(!result);
    }

    #[test]
    fn test_returns_true_if_quorum_reached() {
        let mut messages = HashMap::new();
        messages.insert(
            3,
            vec![
                make_msg(3, HotStuffMessageType::NewView),
                make_msg(3, HotStuffMessageType::NewView),
                make_msg(3, HotStuffMessageType::Prepare),
            ],
        );
        let result = has_quorum_for_view(&messages, 3, 2, HotStuffMessageType::NewView);
        assert!(result);
    }

    #[test]
    fn test_ignores_messages_from_other_views() {
        let mut messages = HashMap::new();
        messages.insert(
            4,
            vec![
                make_msg(3, HotStuffMessageType::NewView), // different view
                make_msg(3, HotStuffMessageType::NewView),
                make_msg(3, HotStuffMessageType::NewView),
            ],
        );
        let result = has_quorum_for_view(&messages, 4, 2, HotStuffMessageType::NewView);
        assert!(!result);
    }

    #[test]
    fn test_filters_by_message_type() {
        let mut messages = HashMap::new();
        messages.insert(
            5,
            vec![
                make_msg(5, HotStuffMessageType::Prepare),
                make_msg(5, HotStuffMessageType::Prepare),
                make_msg(5, HotStuffMessageType::NewView),
            ],
        );
        assert!(has_quorum_for_view(
            &messages,
            5,
            2,
            HotStuffMessageType::Prepare
        ));
        assert!(!has_quorum_for_view(
            &messages,
            5,
            2,
            HotStuffMessageType::NewView
        ));
    }
}
