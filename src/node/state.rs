use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
};

use tokio::{
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::{Mutex, RwLock},
};

use crate::types::Transaction;

pub type PeerId = usize;

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

pub struct Node {
    pub(super) id: PeerId,
    pub(super) transactions: Mutex<Vec<Transaction>>,
    pub(super) seen_transactions: Mutex<HashSet<[u8; 32]>>,
    pub(super) socket_peer_map: RwLock<HashMap<SocketAddr, PeerId>>,
    pub(super) peer_connections: RwLock<HashMap<PeerId, Arc<PeerSocket>>>, // For now, we skip peer discovery
    pub(super) logger: Arc<dyn Logger>,
}
