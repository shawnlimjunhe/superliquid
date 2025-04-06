use tokio::net::TcpStream;

use crate::message_protocol;
use crate::types::Transaction;

pub async fn run_client(addr: &str) -> std::io::Result<()> {
    let tx = Transaction {
        from: "alice".to_string(),
        to: "bob".to_string(),
        amount: 10,
    };

    let mut stream = TcpStream::connect(addr).await?;

    println!("Connected to node");

    let _ = message_protocol::send_transaction(&mut stream, tx).await?;

    println!("Sent transactions");

    let txs: Vec<Transaction> = message_protocol::send_query(&mut stream).await?;

    println!("Recieved Transactions: {:?}", txs);

    println!("Ending connection.");

    message_protocol::send_end(&mut stream).await?;

    Ok(())
}
