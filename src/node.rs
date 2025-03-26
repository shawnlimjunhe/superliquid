use tokio::net::{ TcpListener, TcpStream };
use std::io::Result;
use std::sync::{ Arc, Mutex };

use crate::types::Transaction;
use crate::message_protocol;

struct Node {
    is_leader: bool,
    transactions: Vec<Transaction>,
}

pub async fn run_node(addr: &str) -> Result<()> {
    // Bind the listener to the address
    let listener = TcpListener::bind(addr).await?;
    let node = Arc::new(
        Mutex::new(Node {
            is_leader: true,
            transactions: vec![],
        })
    );

    println!("Listening on addr: {:?}", addr);
    loop {
        let (socket, _) = listener.accept().await?;
        let node = node.clone();
        tokio::spawn(async move {
            match process(socket, node).await {
                Ok(()) => println!("Success"),
                Err(e) => println!("Failed due to: {:?}", e),
            }
        });
    }
}

async fn process(mut socket: TcpStream, node: Arc<Mutex<Node>>) -> std::io::Result<()> {
    let transaction = message_protocol::listen_for_transaction(&mut socket).await?;
    println!("Received Transaction: {:?}", transaction);

    {
        let mut node: std::sync::MutexGuard<'_, Node> = node.lock().expect("Lock failed");
        node.transactions.push(transaction);
    }

    Ok(())
}
