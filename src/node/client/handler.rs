use std::io::Result;
use std::sync::Arc;

use tokio::{
    net::tcp::OwnedWriteHalf,
    sync::{Mutex, mpsc, oneshot},
};

use crate::{
    message_protocol::{self, AppMessage, ControlMessage},
    node::{peer::broadcast::broadcast_transaction, state::Node},
    state::{asset::AssetId, state::AccountInfoWithBalances},
    types::{
        message::{Message, ReplicaInBound, mpsc_error},
        transaction::{PublicKeyHash, SignedTransaction, TransferTransaction, UnsignedTransaction},
    },
};

use super::listener::ClientSocket;

pub struct ClientQuery {
    pub account: PublicKeyHash,
}

pub struct ClientResponse {
    pub account_info_with_balances: AccountInfoWithBalances,
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
            Some(Message::Application(AppMessage::Drip(pk, asset_id))) => {
                handle_drip(&node, pk, asset_id, to_replica_tx.clone()).await?
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
    pk_bytes: &PublicKeyHash,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<ClientResponse> {
    let (response_tx, response_rx) = oneshot::channel();

    // Request info from replica to oneshot channel
    to_replica_tx
        .send(ReplicaInBound::Query(QueryRequest {
            query: ClientQuery { account: *pk_bytes },
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
    pk_bytes: PublicKeyHash,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let response = send_query_to_replica(&pk_bytes, to_replica_tx).await?;
    // send to client
    message_protocol::send_message(
        writer,
        &&Message::Application(AppMessage::AccountQueryResponse(
            response.account_info_with_balances,
        )),
    )
    .await
}

pub(super) async fn handle_drip(
    node: &Arc<Node>,
    pk_bytes: PublicKeyHash,
    asset_id: AssetId,
    to_replica_tx: mpsc::Sender<ReplicaInBound>,
) -> Result<()> {
    let mut faucet_key = node.faucet_key.clone();
    let faucet_pk_bytes = faucet_key.verifying_key().to_bytes();

    let response = send_query_to_replica(&faucet_pk_bytes, to_replica_tx.clone()).await?;

    let account_info = response.account_info_with_balances.account_info;

    let drip_txn = UnsignedTransaction::Transfer(TransferTransaction {
        to: pk_bytes,
        from: faucet_pk_bytes,
        amount: 100000,
        asset_id,
        nonce: account_info.expected_nonce,
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
