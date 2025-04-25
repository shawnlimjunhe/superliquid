use std::io::{Error, ErrorKind, Result};

use std::sync::Arc;

use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use crate::message_protocol::{ControlMessage, send_ack};
use crate::node::client::handler::handle_transaction;
use crate::node::logger::Logger;
use crate::node::state::PeerId;
use crate::types::message::{Message, ReplicaInBound, mpsc_error};
use crate::{
    message_protocol::{self, AppMessage},
    node::state::Node,
};

pub(super) async fn handle_handshake(
    reader: Arc<Mutex<OwnedReadHalf>>,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    logger: Arc<dyn Logger>,
) -> Result<PeerId> {
    let first_msg = message_protocol::receive_message(reader).await?;
    let peer_id = match first_msg {
        Some(Message::Connection(ControlMessage::Hello { peer_id })) => {
            logger.log(
                "info",
                &format!("On handshake: Connection established with peer {peer_id}"),
            );
            peer_id
        }
        other => {
            logger.log("Error", &format!("Expected Hello msg, got: {:?}", other));
            return Err(Error::new(ErrorKind::InvalidData, "Expected Hello Message"));
        }
    };
    send_ack(writer).await?;

    Ok(peer_id)
}

pub(super) async fn handle_peer_connection(
    node: &Arc<Node>,
    reader: Arc<Mutex<OwnedReadHalf>>,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let logger = node.logger.clone();

    loop {
        let message = message_protocol::receive_message(reader.clone()).await;
        match message {
            Ok(Some(Message::HotStuff(hot_stuff_message))) => {
                to_replica_tx
                    .send(ReplicaInBound::HotStuff(hot_stuff_message))
                    .await
                    .map_err(|e| mpsc_error("Send to replica failed", e))?;
            }
            Ok(Some(Message::Application(app_message))) => match app_message {
                AppMessage::SubmitTransaction(signed_tx) => {
                    handle_transaction(&node.clone(), signed_tx, to_replica_tx.clone()).await?;
                }
                AppMessage::Ack => (),
                _ => logger.log(
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
            Ok(None) => {
                logger.log("Error", "Expected message, but got none");
                return Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Expected message but got None instead",
                ));
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}
