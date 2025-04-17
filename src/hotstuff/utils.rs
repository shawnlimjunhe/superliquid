use super::{
    crypto::QuorumCertificate,
    message::{self, HotStuffMessage},
    message_window::MessageWindow,
    replica::ViewNumber,
};

pub fn get_highest_qc_from_votes<'a>(votes: &'a MessageWindow) -> Option<&'a QuorumCertificate> {
    votes
        .iter()
        .filter_map(|msg| msg.justify.as_ref())
        .max_by_key(|qc| qc.view_number)
}

pub(crate) fn has_quorum_for_view(
    messages: Option<&Vec<HotStuffMessage>>,
    view: ViewNumber,
    quorum_threhold: usize,
) -> bool {
    let Some(msgs) = messages else {
        return false;
    };

    return msgs.iter().filter(|m| m.view_number == view).count() >= quorum_threhold;
}
