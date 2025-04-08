use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::message_protocol;
use crate::types::Transaction;

pub async fn run_client(addr: &str) -> std::io::Result<()> {
    let tx = Transaction {
        from: "alice".to_string(),
        to: "bob".to_string(),
        amount: 10,
    };

    let stream = TcpStream::connect(addr).await?;

    let mut stream = Arc::new(Mutex::new(stream));

    println!("Connected to node");

    let _ = message_protocol::send_transaction(&mut stream, tx).await?;

    println!("Sent transactions");

    let txs_opt: Option<Vec<Transaction>> = message_protocol::send_query(&mut stream).await?;

    match txs_opt {
        Some(txs) => println!("Recieved Transactions: {:?}", txs),
        None => println!("Recieved no Transaction"),
    }

    println!("Ending connection.");

    message_protocol::send_end(&mut stream).await?;

    Ok(())
}
