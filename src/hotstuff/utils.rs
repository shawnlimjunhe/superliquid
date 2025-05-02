use super::{
    crypto::QuorumCertificate, message::HotStuffMessage, message_window::MessageWindow,
    replica::ViewNumber,
};

use ed25519::Signature;
use hex::{decode as hex_decode, encode as hex_encode};

pub fn get_highest_qc_from_votes<'a>(votes: &'a MessageWindow) -> Option<&'a QuorumCertificate> {
    votes
        .iter()
        .filter_map(|msg| msg.justify.as_ref())
        .max_by_key(|&qc| qc.view_number)
}

pub(crate) fn has_quorum_votes_for_view(
    messages: Option<&Vec<HotStuffMessage>>,
    view: ViewNumber,
    quorum_threhold: usize,
) -> bool {
    let Some(msgs) = messages else {
        return false;
    };

    return msgs
        .iter()
        .filter(|m| m.view_number == view && m.partial_sig != None)
        .count()
        >= quorum_threhold;
}

pub(crate) fn has_quorum_for_new_view(
    messages: Option<&Vec<HotStuffMessage>>,
    view: ViewNumber,
    quorum_threhold: usize,
) -> bool {
    let Some(msgs) = messages else {
        return false;
    };

    return msgs
        .iter()
        .filter(|m| m.view_number == view && m.justify != None)
        .count()
        >= quorum_threhold;
}

pub(crate) fn sig_to_string(sig: &Signature) -> String {
    hex_encode(sig.to_bytes()) // outputs lowercase hex
}

pub(crate) fn string_to_sig(s: &str) -> Result<Signature, &'static str> {
    let bytes = hex_decode(s).map_err(|_| "Invalid hex")?;
    let array: [u8; 64] = bytes.try_into().map_err(|_| "Invalid sig length")?;
    Ok(Signature::from_bytes(&array))
}
