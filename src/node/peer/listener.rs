use std::sync::Arc;

use std::io::Result;
use tokio::sync::Mutex;
use tokio::{net::TcpListener, sync::mpsc};

use crate::node::peer::handler::handle_handshake;
use crate::node::state::PeerSocket;
use crate::{
    node::{
        peer::handler::handle_peer_connection, runner::deduplicate_peer_connection, state::Node,
    },
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
    let logger = node.logger.clone();
    logger.log(
        "Info",
        &format!("Listening to peers on {:?}", concensus_addr),
    );

    loop {
        let (stream, _) = peer_listener.accept().await?;
        let tx_clone = to_replica_tx.clone();
        let node_clone = node.clone();
        logger.log("Info", "Spawning peer listener");
        let logger = logger.clone();

        let (reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let reader = Arc::new(Mutex::new(reader));

        let peer_socket = Arc::new(PeerSocket::new(reader, writer));

        let peer_id = handle_handshake(
            peer_socket.reader.clone(),
            peer_socket.writer.clone(),
            logger.clone(),
        )
        .await?;

        let peer_socket = deduplicate_peer_connection(peer_socket, &node, peer_id).await;

        tokio::spawn(async move {
            match handle_peer_connection(&node_clone, peer_socket.reader.clone(), tx_clone).await {
                Ok(()) => logger.log("info", "Successfully handled peer connection"),
                Err(e) => logger.log("Error", &format!("Peer listener: Failed due to: {:?}", e)),
            }
        });
    }
}
