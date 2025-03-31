use tokio::{
    net::{TcpListener, TcpStream},
    sync,
};

use std::collections::{HashMap, HashSet};
use std::io::Result;
use std::sync::{Arc, Mutex};

use crate::{
    hotstuff::message::HotStuffMessage,
    message_protocol::{self, AppMessage, send_message, send_transaction},
    types::Message,
};
use crate::{hotstuff::replica::HotStuffReplica, node, types::Transaction};

struct Node {
    id: String,
    _is_leader: bool,
    transactions: Vec<Transaction>,
    seen_transactions: HashSet<[u8; 32]>,
    peers: Vec<String>,
    peer_connections: HashMap<String, Arc<sync::Mutex<TcpStream>>>, // For now, we skip peer discovery
    pub replica: HotStuffReplica,
}

pub async fn run_node(addr: &str, peers: Vec<String>, node_index: usize) -> Result<()> {
    // Bind the listener to the address
    let listener = TcpListener::bind(addr).await?;
    let peer_connections = connect_to_peers(&peers).await?;

    let node = Arc::new(Mutex::new(Node {
        id: format!("node-{}", node_index),
        _is_leader: true,
        transactions: vec![],
        seen_transactions: HashSet::new(),
        peers: peers,
        peer_connections: peer_connections,
        replica: HotStuffReplica::new(node_index),
    }));

    println!("Listening on addr: {:?}", addr);
    loop {
        let (socket, _) = listener.accept().await?;
        let node = node.clone();
        tokio::spawn(async move {
            match handle_connection(socket, node).await {
                Ok(()) => println!("Success"),
                Err(e) => println!("Failed due to: {:?}", e),
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, node: Arc<Mutex<Node>>) -> Result<()> {
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
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn broadcast_hotstuff_message(node: &Arc<Mutex<Node>>, msg: HotStuffMessage) -> Result<()> {
    let peer_connections: Vec<Arc<sync::Mutex<TcpStream>>> = {
        let node = node.lock().expect("Lock failed");
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

async fn send_to(mut socket: &mut TcpStream, msg: HotStuffMessage, peer_id: usize) {
    todo!()
}

async fn connect_to_peers(
    peers: &Vec<String>,
) -> Result<HashMap<String, Arc<sync::Mutex<TcpStream>>>> {
    let mut peer_connections = HashMap::new();

    for peer_addr in peers.iter() {
        match TcpStream::connect(peer_addr).await {
            Ok(stream) => {
                peer_connections.insert(peer_addr.clone(), Arc::new(sync::Mutex::new(stream)));
                println!("Connected to peer at {}", peer_addr);
            }
            Err(e) => {
                eprintln!("Failed to connect to {}: {:?}", peer_addr, e);
            }
        }
    }
    Ok(peer_connections)
}

async fn handle_transaction(
    mut socket: &mut TcpStream,
    node: &Arc<Mutex<Node>>,
    tx: Transaction,
) -> Result<()> {
    println!("Received Transaction: {:?}", tx);
    {
        let mut node = node.lock().expect("Lock failed");

        if node.seen_transactions.insert(tx.hash()) {
            node.transactions.push(tx.clone());
        }
    }
    message_protocol::send_ack(&mut socket).await?;

    let peer_connections: Vec<Arc<sync::Mutex<TcpStream>>> = {
        let node = node.lock().expect("Lock failed");
        node.peer_connections.values().cloned().collect()
    };

    for stream in peer_connections {
        let cloned_tx = tx.clone();
        tokio::spawn(async move {
            let mut stream = stream.lock().await;
            send_transaction(&mut stream, cloned_tx).await
        });
    }

    Ok(())
}

async fn handle_query(mut socket: &mut TcpStream, node: &Arc<Mutex<Node>>) -> Result<()> {
    println!("Received a query");
    let txs = {
        let node = node.lock().expect("Lock failed");
        node.transactions.clone()
    };
    message_protocol::send_message(
        &mut socket,
        &&Message::Application(AppMessage::Response(txs)),
    )
    .await
}
