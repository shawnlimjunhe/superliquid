use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

pub struct ClientConnection {
    pub reader: Arc<Mutex<OwnedReadHalf>>,
    pub writer: Arc<Mutex<OwnedWriteHalf>>,
}

impl ClientConnection {
    pub async fn create_client_connection(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;

        let (reader, writer) = stream.into_split();
        let reader = Arc::new(Mutex::new(reader));
        let writer = Arc::new(Mutex::new(writer));

        Ok(Self { reader, writer })
    }
}
