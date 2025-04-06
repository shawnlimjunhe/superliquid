use std::io::Result;
use std::sync::Arc;

use tokio::{net::TcpStream, sync::mpsc};

use crate::{
    message_protocol::{self, AppMessage},
    node::state::Node,
    types::{Message, ReplicaInBound, mpsc_error},
};

use super::broadcast::broadcast_transaction;

pub(super) async fn handle_peer_connection(
    node: Arc<Node>,
    mut socket: TcpStream,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    loop {
        let message = message_protocol::receive_message(&mut socket).await?;
        match message {
            Message::HotStuff(hot_stuff_message) => {
                to_replica_tx
                    .send(ReplicaInBound::HotStuff(hot_stuff_message))
                    .await
                    .map_err(|e| mpsc_error("Send to replica failed", e))?;
            }
            Message::Application(app_message) => match app_message {
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
                _ => eprint!("Unexpected message on peer connection: {:?}", app_message),
            },
        }
    }
}
