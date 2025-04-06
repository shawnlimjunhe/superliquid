use std::io::{Error, ErrorKind, Result};

use std::sync::Arc;
use std::time;

use chrono::Duration;
use tokio::time::sleep;
use tokio::{net::TcpStream, sync::mpsc};

use crate::node::runner::connect_to_peer;
use crate::node::state::PeerId;
use crate::{
    message_protocol::{self, AppMessage},
    node::state::Node,
    types::{Message, ReplicaInBound, mpsc_error},
};

use super::broadcast::broadcast_transaction;

fn get_peer_info(node: &Arc<Node>, peer_id: PeerId) -> Option<String> {
    node.peers
        .iter()
        .find(|p| p.peer_id == peer_id)
        .map(|p| p.peer_addr.clone())
}

pub(super) async fn handle_peer_connection(
    node: &Arc<Node>,
    mut socket: TcpStream,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let first_msg = message_protocol::receive_message(&mut socket).await?;
    let log = &node.log;
    let peer_id = match first_msg {
        Message::Application(AppMessage::Hello { peer_id }) => {
            log(
                "info",
                &format!("Connection established with peer {peer_id}"),
            );
            peer_id
        }
        other => {
            log("Error", &format!("Expected Hello msg, got: {:?}", other));
            return Err(Error::new(ErrorKind::InvalidData, "Expected Hello Message"));
        }
    };
    loop {
        let message = message_protocol::receive_message(&mut socket).await;
        match message {
            Ok(Message::HotStuff(hot_stuff_message)) => {
                to_replica_tx
                    .send(ReplicaInBound::HotStuff(hot_stuff_message))
                    .await
                    .map_err(|e| mpsc_error("Send to replica failed", e))?;
            }
            Ok(Message::Application(app_message)) => match app_message {
                AppMessage::SubmitTransaction(tx) => {
                    {
                        let mut seen_transactions = node.seen_transactions.lock().await;
                        if seen_transactions.insert(tx.hash()) {
                            {
                                let mut transactions = node.transactions.lock().await;
                                transactions.push(tx.clone());
                            }
                        } else {
                            return Ok(());
                        }
                    }
                    broadcast_transaction(&node, tx.clone()).await?;
                    to_replica_tx
                        .send(ReplicaInBound::Transaction(tx))
                        .await
                        .map_err(|e| mpsc_error("Send to replica failed", e))?;
                }
                AppMessage::Ack => (),
                _ => log(
                    "Error",
                    &format!("Unexpected message on peer connection: {:?}", app_message),
                ),
            },
            Err(e) => {
                log("Error", &format!("Peer {} disconnected: {:?}", peer_id, e));
                {
                    let mut peer_connections = node.peer_connections.lock().await;
                    peer_connections.remove(&peer_id);
                    log("info", "Closing old connection to peer");
                }
                sleep(time::Duration::from_millis(500)).await;
                match get_peer_info(node, peer_id) {
                    Some(peer_addr) => {
                        connect_to_peer(peer_addr, peer_id, node.clone(), node.log.clone()).await;
                    }
                    None => {
                        return Err(Error::new(
                            ErrorKind::NotFound,
                            format!("Peer address not found for peer id: {}", peer_id),
                        ));
                    }
                }
            }
        }
    }
}
