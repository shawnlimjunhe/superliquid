use tokio::{ net::{ TcpListener, TcpStream }, sync };
use std::collections::{ HashMap, HashSet };
use std::io::Result;
use std::sync::{ Arc, Mutex };

use crate::types::Transaction;
use crate::message_protocol::{ self, send_transaction, Message };

struct Node {
    addr: String,
    _is_leader: bool,
    transactions: Vec<Transaction>,
    seen_transactions: HashSet<[u8; 32]>,
    peers: Vec<String>,
    peer_connections: HashMap<String, Arc<sync::Mutex<TcpStream>>>, // For now, we skip peer discovery
}

pub async fn run_node(addr: &str, peers: Vec<String>) -> Result<()> {
    // Bind the listener to the address
    let listener = TcpListener::bind(addr).await?;
    let peer_connections = connect_to_peers(&peers).await?;
    let node = Arc::new(
        Mutex::new(Node {
            addr: addr.to_owned(),
            _is_leader: true,
            transactions: vec![],
            seen_transactions: HashSet::new(),
            peers: peers,
            peer_connections: peer_connections,
        })
    );

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
            Message::Transaction(tx) => {
                handle_transaction(&mut socket, &node, tx).await?;
            }
            Message::Query => {
                handle_query(&mut socket, &node).await?;
            }
            Message::End => {
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn connect_to_peers(
    peers: &Vec<String>
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
    tx: Transaction
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
    message_protocol::send_message(&mut socket, &Message::Response(txs)).await
}
