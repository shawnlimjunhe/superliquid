use std::io::Result;
use std::sync::Arc;

use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc},
};

use crate::{
    node::{client::handler::handle_client_connection, state::Node},
    types::ReplicaInBound,
};

/// client listener handles the application level communication
pub(crate) async fn run_client_listener(
    client_addr: String,
    node: Arc<Node>,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let client_listener: TcpListener = TcpListener::bind(&client_addr).await?;
    let log = node.log.clone();

    log("info", &format!("Listening to client on {:?}", client_addr));

    loop {
        let (socket, _) = client_listener.accept().await?;
        let node = node.clone();
        let to_replica_tx = to_replica_tx.clone();
        let log = log.clone();
        let socket = Arc::new(Mutex::new(socket));
        tokio::spawn(async move {
            match handle_client_connection(socket, node, to_replica_tx).await {
                Ok(()) => log("info", "Successfully handled client connection"),
                Err(e) => log("info", &format!("Client Listener: Failed due to: {:?}", e)),
            }
        });
    }
}
