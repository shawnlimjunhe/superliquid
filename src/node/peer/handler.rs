use std::io::{Error, ErrorKind, Result};

use std::sync::Arc;

use chrono::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio::{net::TcpStream, sync::mpsc};

use crate::message_protocol::ControlMessage;
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

pub(super) async fn handle_handshake(
    socket: Arc<Mutex<TcpStream>>,
    node: &Arc<Node>,
) -> Result<Option<PeerId>> {
    let first_msg = message_protocol::receive_message(&socket).await?;
    let log = node.log.clone();
    let peer_id = match first_msg {
        Some(Message::Connection(ControlMessage::Hello { peer_id })) => {
            log(
                "info",
                &format!("On handshake: Connection established with peer {peer_id}"),
            );
            peer_id
        }
        None => {
            log(
                "info",
                "Expected Hello msg, but connection might have been dropped by peer",
            );
            return Ok(None);
        }
        other => {
            log("Error", &format!("Expected Hello msg, got: {:?}", other));
            return Err(Error::new(ErrorKind::InvalidData, "Expected Hello Message"));
        }
    };

    Ok(Some(peer_id))
}

pub(super) async fn handle_peer_connection(
    node: &Arc<Node>,
    socket: Arc<Mutex<TcpStream>>,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let log = node.log.clone();
    loop {
        let message = message_protocol::receive_message(&socket).await;
        match message {
            Ok(Some(Message::HotStuff(hot_stuff_message))) => {
                to_replica_tx
                    .send(ReplicaInBound::HotStuff(hot_stuff_message))
                    .await
                    .map_err(|e| mpsc_error("Send to replica failed", e))?;
            }
            Ok(Some(Message::Application(app_message))) => match app_message {
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
            Ok(Some(Message::Connection(control_message))) => match control_message {
                ControlMessage::Hello { .. } => {
                    // can discard any new hello messages
                }
                _ => {}
            },
            Ok(None) => log("Error", "Expected message, but got none"),
            Err(e) => {
                // log("Error", &format!("Peer {} disconnected: {:?}", peer_id, e));
                // {
                //     let mut peer_connections = node.peer_connections.lock().await;
                //     peer_connections.remove(&peer_id);
                //     log("info", "Closing old connection to peer");
                // }
                return Err(e);
                // sleep(time::Duration::from_millis(500)).await;

                // match get_peer_info(node, peer_id) {
                //     Some(peer_addr) => {
                //         connect_to_peer(peer_addr, peer_id, node.clone(), node.log.clone()).await;
                //     }
                //     None => {
                //         return Err(Error::new(
                //             ErrorKind::NotFound,
                //             format!("Peer address not found for peer id: {}", peer_id),
                //         ));
                //     }
                // }
            }
        }
    }
}
