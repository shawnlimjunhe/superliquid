use std::{ collections::{ HashMap, HashSet }, net::SocketAddr, sync::Arc };

use tokio::{ net::TcpStream, sync::{ Mutex, RwLock } };

use crate::types::Transaction;

pub type PeerId = usize;
use chrono::Local;

use super::logger::Logger;

pub struct PeerInfo {
    pub peer_id: PeerId,
    pub peer_addr: String,
}

pub struct Node {
    pub(super) id: PeerId,
    pub(super) transactions: Mutex<Vec<Transaction>>,
    pub(super) seen_transactions: Mutex<HashSet<[u8; 32]>>,
    pub(super) socket_peer_map: RwLock<HashMap<SocketAddr, PeerId>>,
    pub(super) peer_connections: RwLock<HashMap<PeerId, Arc<Mutex<TcpStream>>>>, // For now, we skip peer discovery
    pub(super) logger: Arc<dyn Logger>,
}
