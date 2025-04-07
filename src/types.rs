use std::io;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    hotstuff::message::HotStuffMessage,
    message_protocol::{AppMessage, ControlMessage},
    node::state::PeerId,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Transaction {
    pub from: String,
    pub to: String,
    pub amount: u64,
}

pub type Sha256Hash = [u8; 32];

impl Transaction {
    pub fn hash(&self) -> Sha256Hash {
        // probably want to implement my own encoding and hashing
        let encoded = bincode::serialize(&self).unwrap();
        Sha256::digest(&encoded).into()
    }
}

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
    Transaction(Transaction),
}

pub fn mpsc_error<E: std::fmt::Display>(context: &str, err: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{}: {}", context, err))
}
