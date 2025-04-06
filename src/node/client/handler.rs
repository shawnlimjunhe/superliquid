use std::io::Result;
use std::sync::Arc;

use tokio::{net::TcpStream, sync::mpsc};

use crate::{
    message_protocol::{self, AppMessage},
    node::{peer::broadcast::broadcast_transaction, state::Node},
    types::{Message, ReplicaInBound, Transaction, mpsc_error},
};

pub(super) async fn handle_client_connection(
    mut socket: TcpStream,
    node: Arc<Node>,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    loop {
        let message = message_protocol::receive_message(&mut socket).await?;
        match message {
            Message::Application(AppMessage::SubmitTransaction(tx)) => {
                handle_transaction(&mut socket, &node, tx, to_replica_tx.clone()).await?;
            }
            Message::Application(AppMessage::Query) => {
                handle_query(&mut socket, &node).await?;
            }
            Message::Application(AppMessage::End) => {
                return Ok(());
            }
            _ => {}
        }
    }
}

pub(super) async fn handle_transaction(
    socket: &mut TcpStream,
    node: &Arc<Node>,
    tx: Transaction,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let log = node.log.clone();
    log(
        "info",
        &format!(
            "Received Transaction: {:?} on addr: {:?}",
            tx,
            socket.local_addr()
        ),
    );

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

    broadcast_transaction(node, tx.clone()).await?;
    to_replica_tx
        .send(ReplicaInBound::Transaction(tx))
        .await
        .map_err(|e| mpsc_error("Send to replica failed", e))?;

    Ok(())
}

pub(super) async fn handle_query(mut socket: &mut TcpStream, node: &Arc<Node>) -> Result<()> {
    let log = node.log.clone();

    log(
        "info",
        &format!("Received a query from {:?}", socket.peer_addr()),
    );
    let txs = {
        let transactions = node.transactions.lock().await;
        transactions.clone()
    };
    message_protocol::send_message(
        &mut socket,
        &&Message::Application(AppMessage::Response(txs)),
    )
    .await
}
