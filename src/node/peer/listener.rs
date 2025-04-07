use std::sync::Arc;

use std::io::{Error, ErrorKind, Result};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::{net::TcpListener, sync::mpsc};

use crate::node::peer::handler::handle_handshake;
use crate::{
    node::{
        peer::handler::handle_peer_connection, runner::deduplicate_peer_connection, state::Node,
    },
    types::ReplicaInBound,
};

async fn deduplicate_peer_connection_as_listener(
    stream: Arc<Mutex<TcpStream>>,
    node: &Arc<Node>,
) -> Option<Arc<Mutex<TcpStream>>> {
    let socket_addr = {
        let stream = stream.lock().await;
        let socket_addr = stream.peer_addr();
        let socket_addr = match socket_addr {
            Ok(addr) => addr,
            Err(_) => return None,
        };
        socket_addr
    };

    let peer_id = {
        let socket_peer_map = node.socket_peer_map.lock().await;

        socket_peer_map.get(&socket_addr).copied()
    };

    match peer_id {
        Some(peer_id) => {
            // Already have an ongoing connection
            return Some(deduplicate_peer_connection(stream, node, peer_id).await);
        }
        None => {
            let peer_id = match handle_handshake(stream.clone(), node).await {
                Ok(peerid) => peerid,
                Err(_) => return None,
            };
            {
                let mut socket_peer_map = node.socket_peer_map.lock().await;
                socket_peer_map.insert(socket_addr, peer_id);
            }

            return Some(deduplicate_peer_connection(stream, node, peer_id).await);
        }
    }
}

/// peer listener handles the consensus layer communication
pub(crate) async fn run_peer_listener(
    node: Arc<Node>,
    concensus_addr: String,
    // peer: PeerInfo,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let peer_listener: TcpListener = TcpListener::bind(&concensus_addr).await?;
    let log = node.log.clone();
    log(
        "Info",
        &format!("Listening to peers on {:?}", concensus_addr),
    );

    loop {
        let (stream, _) = peer_listener.accept().await?;
        let tx_clone = to_replica_tx.clone();
        let node_clone = node.clone();
        log("Info", "Spawning peer listener");
        let log = log.clone();
        let stream = Arc::new(Mutex::new(stream));
        let stream = deduplicate_peer_connection_as_listener(stream.clone(), &node).await;

        let stream = match stream {
            Some(stream) => stream,
            None => {
                return Err(Error::new(
                    ErrorKind::NotConnected,
                    "could not deduplicate peer connection properly",
                ));
            }
        };

        tokio::spawn(async move {
            match handle_peer_connection(&node_clone, stream, tx_clone).await {
                Ok(()) => log("info", "Successfully handled peer connection"),
                Err(e) => log("Error", &format!("Peer listener: Failed due to: {:?}", e)),
            }
        });
    }
}
