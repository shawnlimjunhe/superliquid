use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::message_protocol;
use crate::types::transaction::UnsignedTransaction;

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
    let tx = UnsignedTransaction {
        from: "alice".to_string(),
        to: "bob".to_string(),
        amount: 10,
    };

    let stream = TcpStream::connect(addr).await?;

    let (reader, writer) = stream.into_split();
    let reader = Arc::new(Mutex::new(reader));
    let writer = Arc::new(Mutex::new(writer));

    message_protocol::send_end(writer).await?;

    Ok(())
}
