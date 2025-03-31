use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{hotstuff::message::HotStuffMessage, message_protocol::AppMessage};

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
    Application(AppMessage),
    HotStuff(HotStuffMessage),
}
