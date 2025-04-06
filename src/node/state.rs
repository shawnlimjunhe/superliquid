use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tokio::{net::TcpStream, sync::Mutex};

use crate::types::Transaction;

pub type PeerId = usize;
use chrono::Local;

pub struct PeerInfo {
    pub peer_id: PeerId,
    pub peer_addr: String,
}

pub type NodeLogger = Arc<dyn Fn(&str, &str) + Send + Sync>;

pub struct Node {
    pub(super) id: PeerId,
    pub(super) peers: Arc<Vec<PeerInfo>>,
    pub(super) transactions: Mutex<Vec<Transaction>>,
    pub(super) seen_transactions: Mutex<HashSet<[u8; 32]>>,
    pub(super) peer_connections: Mutex<HashMap<PeerId, Arc<Mutex<TcpStream>>>>, // For now, we skip peer discovery
    pub(super) log: NodeLogger,
}

pub(crate) fn node_logger(node_id: usize) -> NodeLogger {
    Arc::new(move |level: &str, msg: &str| {
        let now = Local::now().format("%H:%M:%S%.3f");

        let formatted = format!(
            "\x1b[90m[{}]\x1b[0m \x1b[34m[Node {}]\x1b[0m \x1b[93m[{}]\x1b[0m {}",
            now,
            node_id,
            level.to_uppercase(),
            msg
        );

        match level {
            "warn" | "error" => eprintln!("{}", formatted),
            _ => println!("{}", formatted),
        }
    })
}
