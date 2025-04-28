use std::io::Result;
use std::sync::Arc;

use tokio::{
    net::tcp::OwnedWriteHalf,
    sync::{Mutex, mpsc, oneshot},
};

use crate::{
    message_protocol::{self, AppMessage, ControlMessage},
    node::{peer::broadcast::broadcast_transaction, state::Node},
    state::state::AccountInfo,
    types::{
        message::{Message, ReplicaInBound, mpsc_error},
        transaction::{
            PublicKeyString, SignedTransaction, TransferTransaction, UnsignedTransaction,
        },
    },
};

use super::listener::ClientSocket;

pub struct ClientQuery {
    pub account: PublicKeyString,
}

pub struct ClientResponse {
    pub account_info: AccountInfo,
}

pub struct QueryRequest {
    pub query: ClientQuery,
    pub response_channel: oneshot::Sender<ClientResponse>,
}

pub(super) async fn handle_client_connection(
    socket: ClientSocket,
    node: Arc<Node>,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    loop {
        let message = message_protocol::receive_message(socket.reader.clone()).await?;
        match message {
            Some(Message::Application(AppMessage::SubmitTransaction(tx))) => {
                handle_transaction(&node, tx, to_replica_tx.clone()).await?;
            }
            Some(Message::Application(AppMessage::Query)) => {
                handle_query(socket.writer.clone(), &node).await?;
            }
            Some(Message::Application(AppMessage::Drip(pk))) => {
                handle_drip(&node, pk, to_replica_tx.clone()).await?
            }
            Some(Message::Application(AppMessage::AccountQuery(pk))) => {
                handle_account_query(socket.writer.clone(), pk, to_replica_tx.clone()).await?;
            }
            Some(Message::Connection(ControlMessage::End)) => {
                return Ok(());
            }
            _ => {}
        }
    }
}

pub(super) async fn send_query_to_replica(
    pk_hex: &PublicKeyString,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<ClientResponse> {
    let (response_tx, response_rx) = oneshot::channel();

    // Request info from replica to oneshot channel
    to_replica_tx
        .send(ReplicaInBound::Query(QueryRequest {
            query: ClientQuery {
                account: pk_hex.clone(),
            },
            response_channel: response_tx,
        }))
        .await
        .map_err(|e| mpsc_error("Failed to send query request to replica", e))?;

    println!("Waiting for response");
    let response = response_rx
        .await
        .map_err(|e| mpsc_error("Failed to recieve response from replica", e))?;
    return Ok(response);
}

pub(super) async fn handle_account_query(
    writer: Arc<Mutex<OwnedWriteHalf>>,
    pk_hex: PublicKeyString,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let response = send_query_to_replica(&pk_hex, to_replica_tx).await?;
    // send to client
    message_protocol::send_message(
        writer,
        &&Message::Application(AppMessage::AccountQueryResponse(response.account_info)),
    )
    .await
}

pub(super) async fn handle_drip(
    node: &Arc<Node>,
    pk_hex: PublicKeyString,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let mut faucet_key = node.faucet_key.clone();

    let response = send_query_to_replica(&pk_hex, to_replica_tx.clone()).await?;

    let drip_txn = UnsignedTransaction::Transfer(TransferTransaction {
        to: pk_hex,
        from: PublicKeyString::from_public_key(&faucet_key.verifying_key()),
        amount: 100000,
        nonce: response.account_info.nonce + 1,
    });

    let drip_txn = drip_txn.sign(&mut faucet_key);
    handle_transaction(node, drip_txn, to_replica_tx).await
}

pub(crate) async fn handle_transaction(
    node: &Arc<Node>,
    signed_tx: SignedTransaction,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let logger = node.logger.clone();

    signed_tx.verify_sender();

    {
        let mut seen_transactions = node.seen_transactions.lock().await;
        if seen_transactions.insert(signed_tx.hash()) {
            {
                let mut transactions = node.transactions.lock().await;
                transactions.push(signed_tx.clone());
            }
        } else {
            return Ok(());
        }
    }
    logger.log("info", &format!("Received Transaction: {:?}", signed_tx));

    broadcast_transaction(&node, signed_tx.clone()).await?;
    to_replica_tx
        .send(ReplicaInBound::Transaction(signed_tx))
        .await
        .map_err(|e| mpsc_error("Send to replica failed", e))?;

    Ok(())
}

pub(super) async fn handle_query(
    writer: Arc<Mutex<OwnedWriteHalf>>,
    node: &Arc<Node>,
) -> Result<()> {
    let logger = node.logger.clone();

    let peer_addr = { writer.lock().await.peer_addr() };

    logger.log("info", &format!("Received a query from {:?}", peer_addr));
    let txs = {
        let transactions = node.transactions.lock().await;
        transactions.clone()
    };
    message_protocol::send_message(writer, &&Message::Application(AppMessage::Response(txs))).await
}
