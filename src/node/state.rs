use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tokio::{net::TcpStream, sync::Mutex};

use crate::types::Transaction;

pub type PeerId = usize;

pub struct PeerInfo {
    pub peer_id: PeerId,
    pub peer_addr: String,
}

pub struct Node {
    pub(super) id: PeerId,
    pub(super) peers: Arc<Vec<PeerInfo>>,
    pub(super) transactions: Mutex<Vec<Transaction>>,
    pub(super) seen_transactions: Mutex<HashSet<[u8; 32]>>,
    pub(super) peer_connections: Mutex<HashMap<PeerId, Arc<Mutex<TcpStream>>>>, // For now, we skip peer discovery
}
