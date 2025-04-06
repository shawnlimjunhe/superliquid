use std::sync::Arc;

use std::io::Result;
use tokio::{net::TcpListener, sync::mpsc};

use crate::{
    node::{peer::handler::handle_peer_connection, state::Node},
    types::ReplicaInBound,
};

/// peer listener handles the consensus layer communication
pub(crate) async fn run_peer_listener(
    node: Arc<Node>,
    concensus_addr: String,
    // peer: PeerInfo,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let peer_listener: TcpListener = TcpListener::bind(&concensus_addr).await?;
    println!("Listening to peers on {:?}", concensus_addr);

    loop {
        let (socket, _) = peer_listener.accept().await?;
        let tx_clone = to_replica_tx.clone();
        let node_clone = node.clone();
        // let peer_addr = peer.peer_addr.clone();

        println!("Spawning peer listener");

        tokio::spawn(async move {
            match handle_peer_connection(&node_clone, socket, tx_clone).await {
                Ok(()) => println!("Successfully handled peer connection"),
                Err(e) => println!("Failed due to: {:?}", e),
            }
        });
    }
}
