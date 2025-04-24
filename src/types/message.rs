use std::io;

use serde::{Deserialize, Serialize};

use crate::{
    hotstuff::message::HotStuffMessage,
    message_protocol::{AppMessage, ControlMessage},
    node::state::PeerId,
};

use super::transaction::UnsignedTransaction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    Connection(ControlMessage),
    Application(AppMessage),
    HotStuff(HotStuffMessage),
}

pub enum ReplicaOutbound {
    Broadcast(HotStuffMessage),
    SendTo(PeerId, HotStuffMessage),
}

pub enum ReplicaInBound {
    HotStuff(HotStuffMessage),
    Transaction(UnsignedTransaction),
}

pub fn mpsc_error<E: std::fmt::Display>(context: &str, err: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{}: {}", context, err))
}
