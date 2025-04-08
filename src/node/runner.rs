use futures::future::join_all;
use tokio::{
    net::TcpStream,
    sync::{Mutex, RwLock, mpsc},
    time::sleep,
};

use crate::{
    hotstuff::replica::HotStuffReplica,
    message_protocol::send_hello,
    types::{ReplicaInBound, ReplicaOutbound},
};

use super::{
    client::listener::run_client_listener,
    peer::listener::run_peer_listener,
    replica::handle_replica_outbound,
    state::{Node, NodeLogger, PeerId, PeerInfo, node_logger},
};
use std::{
    collections::{HashMap, HashSet},
    io::Result,
    sync::Arc,
    time,
};

async fn spawn_all_node_tasks(
    client_addr: String,
    consensus_addr: String,
    mut replica: HotStuffReplica,
    node: Arc<Node>,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
    to_replica_rx: mpsc::Receiver<ReplicaInBound>,
    from_replica_rx: mpsc::Receiver<ReplicaOutbound>,
) -> Result<()> {
    let handles = vec![
        tokio::spawn(run_client_listener(
            client_addr.to_owned(),
            node.clone(),
            to_replica_tx.clone(),
        )),
        tokio::spawn(run_peer_listener(
            node.clone(),
            consensus_addr.to_owned(),
            to_replica_tx,
        )),
    ];

    tokio::spawn(async move { replica.run_replica(to_replica_rx).await });
    tokio::spawn(handle_replica_outbound(from_replica_rx, node.clone()));

    let _ = join_all(handles).await;
    Ok(())
}

// It is possible for two nodes to establish connections with one another,
// Without  Deduplication, we can have conflicting streams for the same peer.
pub(crate) async fn deduplicate_peer_connection(
    stream: Arc<Mutex<TcpStream>>,
    node: &Arc<Node>,
    peer_id: PeerId,
) -> Arc<Mutex<TcpStream>> {
    let log = node.log.clone();
    let mut peer_connections = node.peer_connections.write().await;
    match peer_connections.get(&peer_id) {
        Some(stream) => {
            log(
                "Info",
                &format!("Deduplicated TCP stream with peer: {:?}", peer_id),
            );
            return stream.clone();
        }
        None => {
            let stream_clone = stream.clone();
            peer_connections.insert(peer_id, stream_clone);
            return stream.clone();
        }
    }
}

pub(crate) async fn connect_to_peer(
    addr: String,
    peer_id: usize,
    node: Arc<Node>,
    log: NodeLogger,
) {
    let base: u32 = 100;
    let mut counts: u32 = 1;
    let max_sleep: u32 = 1000 * 60;
    loop {
        match TcpStream::connect(addr.clone()).await {
            Ok(stream) => {
                log(
                    "info",
                    &format!("Initiate: Connection established with peer {}", peer_id),
                );
                let socket_addr = stream
                    .peer_addr()
                    .expect("Expect stream to have peer address");
                let stream = Arc::new(Mutex::new(stream));

                {
                    let mut socket_peer_map = node.socket_peer_map.write().await;

                    if !socket_peer_map.contains_key(&socket_addr) {
                        socket_peer_map.insert(socket_addr, peer_id);
                    }
                }

                let stream = deduplicate_peer_connection(stream, &node, peer_id).await;
                send_hello(stream, node.id).await.unwrap();
                break;
            }
            Err(e) => {
                log("error", &format!("Failed to connect to {}: {:?}", addr, e));
                let exp_duration = base.pow(counts);
                let sleep_duration = exp_duration.min(max_sleep);
                if sleep_duration != max_sleep {
                    counts = counts.saturating_add(1);
                }

                sleep(time::Duration::from_millis(sleep_duration.into())).await;
            }
        }
    }
}

async fn connect_to_peers_background(peers: &Vec<PeerInfo>, node: &Arc<Node>) {
    for peer_info in peers {
        let node_clone = node.clone();
        let peer_id = peer_info.peer_id;
        let addr = peer_info.peer_addr.clone();
        let log = node.log.clone();
        tokio::spawn(async move {
            connect_to_peer(addr, peer_id, node_clone, log).await;
        });
    }
}

pub async fn run_node(
    client_addr: String,
    consensus_addr: String,
    peers: Vec<PeerInfo>,
    node_index: usize,
) -> Result<()> {
    // Bind the listener to the address
    let logger = node_logger(node_index);

    let peers = Arc::new(peers);
    let node = Arc::new(Node {
        id: node_index,
        transactions: Mutex::new(vec![]),
        peers: peers.clone(),
        seen_transactions: Mutex::new(HashSet::new()),
        peer_connections: RwLock::new(HashMap::new()),
        log: logger.clone(),
        socket_peer_map: RwLock::new(HashMap::new()),
    });
    connect_to_peers_background(&peers, &node).await;

    // Sends messages to replica from node
    let (to_replica_tx, to_replica_rx): (
        mpsc::Sender<ReplicaInBound>,
        mpsc::Receiver<ReplicaInBound>,
    ) = mpsc::channel(1024);

    // Recieves messages from replica to node
    let (from_replica_tx, from_replica_rx): (
        mpsc::Sender<ReplicaOutbound>,
        mpsc::Receiver<ReplicaOutbound>,
    ) = mpsc::channel(1024);

    let replica = HotStuffReplica::new(node_index, from_replica_tx);

    let _ = spawn_all_node_tasks(
        client_addr,
        consensus_addr,
        replica,
        node,
        to_replica_tx,
        to_replica_rx,
        from_replica_rx,
    )
    .await;
    Ok(())
}
