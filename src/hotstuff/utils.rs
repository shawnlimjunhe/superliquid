use std::collections::HashMap;

use super::{crypto::QuorumCertificate, message::HotStuffMessage, replica::ViewNumber};

pub fn get_highest_qc_from_votes<'a>(
    votes: &'a Vec<HotStuffMessage>,
) -> Option<&'a QuorumCertificate> {
    votes
        .iter()
        .filter_map(|msg| msg.justify.as_ref())
        .max_by_key(|qc| qc.view_number)
}

pub(crate) fn has_quorum_for_view(
    messages: &HashMap<ViewNumber, Vec<HotStuffMessage>>,
    view: ViewNumber,
    quorum_threhold: usize,
) -> bool {
    let Some(msgs) = messages.get(&view) else {
        return false;
    };

    return msgs.iter().filter(|m| m.view_number == view).count() >= quorum_threhold;
}

#[cfg(test)]
mod tests {
    use crate::hotstuff::utils::{self};

    #[test]
    fn returns_none_if_no_votes() {
        let votes = vec![];
        let result = utils::get_highest_qc_from_votes(&votes);
        assert!(result.is_none());
    }
}
