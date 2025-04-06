use futures::future::join_all;
use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc},
};

use crate::{
    hotstuff::replica::HotStuffReplica,
    types::{ReplicaInBound, ReplicaOutbound},
};

use super::{
    client::listener::run_client_listener,
    peer::listener::run_peer_listener,
    replica::handle_replica_outbound,
    state::{Node, PeerId, PeerInfo},
};
use std::{
    collections::{HashMap, HashSet},
    io::Result,
    sync::Arc,
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

async fn connect_to_peers(peers: &Vec<PeerInfo>) -> Result<HashMap<PeerId, Arc<Mutex<TcpStream>>>> {
    let mut peer_connections = HashMap::new();

    for peer_info in peers.iter() {
        let peer_addr = &peer_info.peer_addr;
        match TcpStream::connect(peer_addr).await {
            Ok(stream) => {
                peer_connections.insert(peer_info.peer_id, Arc::new(Mutex::new(stream)));
            }
            Err(e) => eprintln!("Failed to connect to {}: {:?}", peer_addr, e),
        }
    }

    println!(
        "Established connection to {:?} peers out of {:?}",
        peer_connections.len(),
        peers.len()
    );
    Ok(peer_connections)
}

pub async fn run_node(
    client_addr: String,
    consensus_addr: String,
    peers: Vec<PeerInfo>,
    node_index: usize,
) -> Result<()> {
    // Bind the listener to the address
    let peer_connections: HashMap<PeerId, Arc<Mutex<TcpStream>>> = connect_to_peers(&peers).await?;

    let node = Arc::new(Node {
        id: node_index,
        transactions: Mutex::new(vec![]),
        seen_transactions: Mutex::new(HashSet::new()),
        peer_connections: Mutex::new(peer_connections),
    });

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
