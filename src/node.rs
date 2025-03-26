use tokio::net::{ TcpListener, TcpStream };
use std::io::Result;
use std::sync::{ Arc, Mutex };

use crate::types::Transaction;
use crate::message_protocol::{ self, Message };

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
            match handle_connection(socket, node).await {
                Ok(()) => println!("Success"),
                Err(e) => println!("Failed due to: {:?}", e),
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, node: Arc<Mutex<Node>>) -> std::io::Result<()> {
    loop {
        let message = message_protocol::receive_message(&mut socket).await?;

        match message {
            Message::Transaction(tx) => {
                println!("Received Transaction: {:?}", tx);
                {
                    let mut node: std::sync::MutexGuard<'_, Node> = node
                        .lock()
                        .expect("Lock failed");
                    node.transactions.push(tx);
                }
                message_protocol::send_ack(&mut socket).await?;
            }

            Message::Query => {
                println!("Received a query");
                let txs = {
                    let node = node.lock().expect("Lock failed");
                    node.transactions.clone()
                };
                message_protocol::send_message(&mut socket, &Message::Response(txs)).await?;
            }
            _ => {}
        }
    }

    Ok(())
}
