use chrono::Duration;
use futures::future::join_all;
use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc},
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
    state::{Node, PeerInfo},
};
use std::{
    collections::{HashMap, HashSet},
    io::Result,
    rc::Rc,
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

pub(crate) async fn connect_to_peer(addr: String, peer_id: usize, node_clone: Arc<Node>) {
    let base: u32 = 100;
    let mut counts: u32 = 1;
    let max_sleep: u32 = 1000 * 60;
    loop {
        match TcpStream::connect(addr.clone()).await {
            Ok(mut stream) => {
                println!("Connected to peer {} at {}", peer_id, addr);
                send_hello(&mut stream, peer_id).await.unwrap();
                let mut peer_connections = node_clone.peer_connections.lock().await;
                peer_connections.insert(peer_id, Arc::new(Mutex::new(stream)));
                break;
            }
            Err(e) => {
                eprintln!("Failed to connect to {}: {:?}", addr, e);
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
        tokio::spawn(async move {
            connect_to_peer(addr, peer_id, node_clone).await;
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
    let peers = Arc::new(peers);
    let node = Arc::new(Node {
        id: node_index,
        transactions: Mutex::new(vec![]),
        peers: peers.clone(),
        seen_transactions: Mutex::new(HashSet::new()),
        peer_connections: Mutex::new(HashMap::new()),
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
