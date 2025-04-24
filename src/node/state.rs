use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
};

use ed25519_dalek::SigningKey;
use tokio::{
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::{Mutex, RwLock},
};

pub type PeerId = usize;

use crate::types::transaction::SignedTransaction;

use super::logger::Logger;

pub struct PeerInfo {
    pub peer_id: PeerId,
    pub peer_addr: String,
}

pub struct PeerSocket {
    pub reader: Arc<Mutex<OwnedReadHalf>>,
    pub writer: Arc<Mutex<OwnedWriteHalf>>,
}

impl PeerSocket {
    pub fn new(reader: Arc<Mutex<OwnedReadHalf>>, writer: Arc<Mutex<OwnedWriteHalf>>) -> Self {
        Self { reader, writer }
    }
}

// Node handles the communication logic with the client and other nodes.
pub struct Node {
    pub(super) id: PeerId,
    pub(crate) faucet_key: SigningKey,
    pub(super) transactions: Mutex<Vec<SignedTransaction>>,
    pub(super) seen_transactions: Mutex<HashSet<[u8; 32]>>,
    pub(super) socket_peer_map: RwLock<HashMap<SocketAddr, PeerId>>,
    pub(super) peer_connections: RwLock<HashMap<PeerId, Arc<PeerSocket>>>, // For now, we skip peer discovery
    pub(super) logger: Arc<dyn Logger>,
}

impl Node {
    pub(crate) async fn get_peer_connections_as_vec(&self) -> Vec<Arc<PeerSocket>> {
        {
            let peer_connections = self.peer_connections.read().await;
            peer_connections.values().cloned().collect()
        }
    }
}
