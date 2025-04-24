use std::io::Result;
use std::sync::Arc;

use tokio::{
    net::{
        TcpListener,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{Mutex, mpsc},
};

use crate::{
    node::{client::handler::handle_client_connection, state::Node},
    types::message::ReplicaInBound,
};

pub struct ClientSocket {
    pub reader: Arc<Mutex<OwnedReadHalf>>,
    pub writer: Arc<Mutex<OwnedWriteHalf>>,
}

impl ClientSocket {
    pub fn new(reader: Arc<Mutex<OwnedReadHalf>>, writer: Arc<Mutex<OwnedWriteHalf>>) -> Self {
        Self { reader, writer }
    }
}

/// client listener handles the application level communication
pub(crate) async fn run_client_listener(
    client_addr: String,
    node: Arc<Node>,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let client_listener: TcpListener = TcpListener::bind(&client_addr).await?;
    let logger = node.logger.clone();

    logger.log("info", &format!("Listening to client on {:?}", client_addr));

    loop {
        let (socket, _) = client_listener.accept().await?;
        let node = node.clone();
        let to_replica_tx = to_replica_tx.clone();
        let logger = node.logger.clone();

        let (reader, writer) = socket.into_split();

        let reader = Arc::new(Mutex::new(reader));
        let writer = Arc::new(Mutex::new(writer));

        let client_socket = ClientSocket::new(reader, writer);

        tokio::spawn(async move {
            match handle_client_connection(client_socket, node, to_replica_tx).await {
                Ok(()) => logger.log("info", "Successfully handled client connection"),
                Err(e) => logger.log("info", &format!("Client Listener: Failed due to: {:?}", e)),
            }
        });
    }
}
