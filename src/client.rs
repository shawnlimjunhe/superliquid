use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::message_protocol;
use crate::types::Transaction;

pub struct ClientConnection {
    pub reader: Arc<Mutex<OwnedReadHalf>>,
    pub writer: Arc<Mutex<OwnedWriteHalf>>,
}

impl ClientConnection {
    async fn create_client_connection(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;

        let (reader, writer) = stream.into_split();
        let reader = Arc::new(Mutex::new(reader));
        let writer = Arc::new(Mutex::new(writer));

        Ok(Self { reader, writer })
    }
}

pub async fn run_client(addr: &str) -> std::io::Result<()> {
    let tx = Transaction {
        from: "alice".to_string(),
        to: "bob".to_string(),
        amount: 10,
    };

    let stream = TcpStream::connect(addr).await?;

    let (reader, writer) = stream.into_split();
    let reader = Arc::new(Mutex::new(reader));
    let writer = Arc::new(Mutex::new(writer));

    println!("Connected to node");

    let _ = message_protocol::send_transaction(writer.clone(), tx).await?;

    println!("Sent transactions");

    let txs_opt: Option<Vec<Transaction>> =
        message_protocol::send_query(reader, writer.clone()).await?;

    match txs_opt {
        Some(txs) => println!("Recieved Transactions: {:?}", txs),
        None => println!("Recieved no Transaction"),
    }

    println!("Ending connection.");

    message_protocol::send_end(writer).await?;

    Ok(())
}
