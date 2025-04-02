use tokio::{ net::{ TcpListener, TcpStream }, sync::{ self, mpsc } };
use futures::future::join_all;
use std::collections::{ HashMap, HashSet };
use std::io::Result;
use std::sync::{ Arc, Mutex };

use crate::{
    hotstuff::message::HotStuffMessage,
    message_protocol::{ self, AppMessage, send_message, send_transaction },
    types::Message,
};
use crate::{ hotstuff::replica::HotStuffReplica, node, types::Transaction };

pub type PeerId = usize;

pub struct PeerInfo {
    pub peer_id: usize,
    pub peer_addr: String,
}

pub enum ReplicaOutbound {
    Broadcast(HotStuffMessage),
    SendTo(PeerId, HotStuffMessage),
}

struct Node {
    id: PeerId,
    _is_leader: bool,
    transactions: Vec<Transaction>,
    seen_transactions: HashSet<[u8; 32]>,
    peers: Vec<PeerInfo>,
    peer_connections: HashMap<PeerId, Arc<sync::Mutex<TcpStream>>>, // For now, we skip peer discovery
    replica_ids: Vec<PeerId>,
}

/// Runs the overall node has listens on two ports, 1 to handle client side connections and
/// another for peer connections
pub async fn run_node(
    client_addr: String,
    consensus_addr: String,
    peers: Vec<PeerInfo>,
    node_index: usize,
    num_validators: usize
) -> Result<()> {
    // Bind the listener to the address
    let peer_connections: HashMap<PeerId, Arc<sync::Mutex<TcpStream>>> = connect_to_peers(
        &peers
    ).await?;

    let node = Arc::new(
        sync::Mutex::new(Node {
            id: node_index,
            _is_leader: true,
            transactions: vec![],
            seen_transactions: HashSet::new(),
            peers: peers,
            peer_connections: peer_connections,
            replica_ids: (0..num_validators).collect(),
        })
    );

    // Sends messages to replica from node
    let (to_replica_tx, to_replica_rx): (
        mpsc::Sender<HotStuffMessage>,
        mpsc::Receiver<HotStuffMessage>,
    ) = mpsc::channel(1024);

    // Recieves messages from replica to node
    let (from_replica_tx, from_replica_rx): (
        mpsc::Sender<ReplicaOutbound>,
        mpsc::Receiver<ReplicaOutbound>,
    ) = mpsc::channel(1024);

    tokio::spawn(run_client_listener(client_addr.to_owned(), node.clone()));
    tokio::spawn(run_peer_listener(consensus_addr.to_owned(), node.clone()));

    Ok(())
}

/// client listener handles the application level communication
async fn run_client_listener(client_addr: String, node: Arc<sync::Mutex<Node>>) -> Result<()> {
    let client_listener: TcpListener = TcpListener::bind(&client_addr).await?;
    println!("Listening to client on {:?}", client_addr);

    loop {
        let (socket, _) = client_listener.accept().await?;
        let node = node.clone();
        tokio::spawn(async move {
            match handle_client_connection(socket, node).await {
                Ok(()) => println!("Successfully handled client connection"),
                Err(e) => println!("Failed due to: {:?}", e),
            }
        });
    }
}

async fn handle_client_connection(
    mut socket: TcpStream,
    node: Arc<sync::Mutex<Node>>
) -> Result<()> {
    loop {
        let message = message_protocol::receive_message(&mut socket).await?;
        match message {
            Message::Application(AppMessage::SubmitTransaction(tx)) => {
                handle_transaction(&mut socket, &node, tx).await?;
            }
            Message::Application(AppMessage::Query) => {
                handle_query(&mut socket, &node).await?;
            }
            Message::Application(AppMessage::End) => {
                return Ok(());
            }
            _ => {}
        }
    }
}

async fn handle_peer_connection(mut socket: TcpStream, node: Arc<sync::Mutex<Node>>) -> Result<()> {
    loop {
        let message = message_protocol::receive_message(&mut socket).await?;
        if let Message::HotStuff(hot_stuff_message) = message {
            {
                let mut node = node.lock().await;
                node.replica.process_message(hot_stuff_message);
            }
            println!("Received HotStuff message");
        } else {
            eprintln!("Unexpected message on peer connection: {:?}", message);
        }
    }
}

/// peer listener handles the consensus layer communication
async fn run_peer_listener(concensus_addr: String, node: Arc<sync::Mutex<Node>>) -> Result<()> {
    let peer_listener: TcpListener = TcpListener::bind(&concensus_addr).await?;
    println!("Listening to peers on {:?}", concensus_addr);

    loop {
        let (socket, _) = peer_listener.accept().await?;
        let node = node.clone();
        println!("Spawning peer listener");
        tokio::spawn(async move {
            match handle_peer_connection(socket, node).await {
                Ok(()) => println!("Successfully handled peer connection"),
                Err(e) => println!("Failed due to: {:?}", e),
            }
        });
    }
}

/// Broadcast msg to all peer connections
async fn _broadcast_hotstuff_message(
    node: &Arc<sync::Mutex<Node>>,
    msg: HotStuffMessage
) -> Result<()> {
    let peer_connections: Vec<Arc<sync::Mutex<TcpStream>>> = {
        let node = node.lock().await;
        node.peer_connections.values().cloned().collect()
    };

    for stream in peer_connections {
        let cloned_msg = msg.clone();
        tokio::spawn(async move {
            let mut stream = stream.lock().await;
            send_message(&mut stream, &Message::HotStuff(cloned_msg)).await
        });
    }

    Ok(())
}

async fn send_to_node(
    node: &Arc<sync::Mutex<Node>>,
    msg: HotStuffMessage,
    peer_id: PeerId
) -> Result<()> {
    let peer_connection = {
        let node = node.lock().await;
        node.peer_connections.get(&peer_id).cloned()
    };

    let Some(peer_connection) = peer_connection else {
        return Ok(());
    };
    {
        let mut peer_connection = peer_connection.lock().await;
        send_message(&mut peer_connection, &Message::HotStuff(msg)).await
    }
}

/// Sends message to the leader for the current view
async fn send_to_leader(node: &Arc<sync::Mutex<Node>>, msg: HotStuffMessage) -> Result<()> {
    let leader = {
        let node = node.lock().await;
        node.replica.pacemaker.current_leader(&node.replica_ids)
    };

    send_to_node(node, msg, leader).await
}

async fn connect_to_peers(
    peers: &Vec<PeerInfo>
) -> Result<HashMap<PeerId, Arc<sync::Mutex<TcpStream>>>> {
    let mut peer_connections = HashMap::new();

    for peer_info in peers.iter() {
        let peer_addr = &peer_info.peer_addr;
        match TcpStream::connect(peer_addr).await {
            Ok(stream) => {
                peer_connections.insert(peer_info.peer_id, Arc::new(sync::Mutex::new(stream)));
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

async fn handle_transaction(
    mut socket: &mut TcpStream,
    node: &Arc<sync::Mutex<Node>>,
    tx: Transaction
) -> Result<()> {
    println!("Received Transaction: {:?} on addr: {:?}", tx, socket.local_addr());
    let id = {
        let mut node = node.lock().await;

        if node.seen_transactions.insert(tx.hash()) {
            node.transactions.push(tx.clone());
        }
        node.id.clone()
    };
    message_protocol::send_ack(&mut socket).await?;

    let peer_connections: Vec<Arc<sync::Mutex<TcpStream>>> = {
        let node = node.lock().await;
        node.peer_connections.values().cloned().collect()
    };

    let mut tasks = Vec::new();

    println!("broadcasting tx from node {} to {} peers", id, peer_connections.len());
    for stream in peer_connections {
        let cloned_tx = tx.clone();
        let task = tokio::spawn(async move {
            let mut stream = stream.lock().await;
            send_transaction(&mut stream, cloned_tx).await
        });
        tasks.push(task);
    }

    let results = join_all(tasks).await;
    for result in results {
        match result {
            Ok(Ok(())) => {
                println!("sent transaction ");
            }
            Ok(Err(e)) => eprintln!("send_transaction error: {:?}", e),
            Err(e) => eprintln!("task panicked: {:?}", e),
        }
    }

    println!("Finish broadcasting tx");

    Ok(())
}

async fn handle_query(mut socket: &mut TcpStream, node: &Arc<sync::Mutex<Node>>) -> Result<()> {
    println!("Received a query on: {:?} from: {:?}", socket.local_addr(), socket.peer_addr());
    let txs = {
        let node = node.lock().await;
        node.transactions.clone()
    };
    message_protocol::send_message(
        &mut socket,
        &&Message::Application(AppMessage::Response(txs))
    ).await
}
