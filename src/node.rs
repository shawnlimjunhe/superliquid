use tokio::net::{ TcpListener, TcpStream };
use std::collections::HashSet;
use std::io::Result;
use std::sync::{ Arc, Mutex };

use crate::types::Transaction;
use crate::message_protocol::{ self, send_transaction, Message };

struct Node {
    addr: String,
    _is_leader: bool,
    transactions: Vec<Transaction>,
    seen_transactions: HashSet<[u8; 32]>,
    peers: Vec<Node>, // For now, we skip peer discovery
}

impl Node {
    pub fn get_peer_addresses(&self) -> Vec<String> {
        self.peers
            .iter()
            .map(|node| node.addr.clone())
            .collect()
    }
}

pub async fn run_node(addr: &str) -> Result<()> {
    // Bind the listener to the address
    let listener = TcpListener::bind(addr).await?;
    let node = Arc::new(
        Mutex::new(Node {
            addr: addr.to_owned(),
            _is_leader: true,
            transactions: vec![],
            seen_transactions: HashSet::new(),
            peers: vec![],
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

    let peer_addresses = {
        let node = node.lock().expect("Lock failed");
        node.get_peer_addresses()
    };

    for addr in peer_addresses.into_iter() {
        let cloned_tx = tx.clone();
        tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await?;
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
