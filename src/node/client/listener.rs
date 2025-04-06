use std::io::Result;
use std::sync::Arc;

use tokio::{net::TcpListener, sync::mpsc};

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
    println!("Listening to client on {:?}", client_addr);

    loop {
        let (socket, _) = client_listener.accept().await?;
        let node = node.clone();
        let to_replica_tx = to_replica_tx.clone();
        tokio::spawn(async move {
            match handle_client_connection(socket, node, to_replica_tx).await {
                Ok(()) => println!("Successfully handled client connection"),
                Err(e) => println!("Failed due to: {:?}", e),
            }
        });
    }
}
