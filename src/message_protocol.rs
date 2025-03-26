use tokio::net::TcpStream;
use serde::{ Serialize, Deserialize };

use std::io::{ Result, Error, ErrorKind };
use crate::types::Transaction;
use crate::network;

#[derive(Serialize, Deserialize, Debug)]
pub enum Message {
    Query,
    Transaction(Transaction),
    Response(Vec<Transaction>),
    Ack,
}

pub async fn receive_message(stream: &mut TcpStream) -> Result<Message> {
    network::receive_json::<Message>(stream).await
}

pub async fn send_message(stream: &mut TcpStream, message: &Message) -> Result<()> {
    let json = serde_json::to_vec(&message)?;
    let _ = network::send_data(stream, &json).await;
    Ok(())
}

pub async fn send_transaction(stream: &mut TcpStream, tx: Transaction) -> Result<()> {
    let msg = Message::Transaction(tx);
    send_message(stream, &msg).await?;

    match network::receive_json::<Message>(stream).await? {
        Message::Ack => Ok(()), // basic ACK
        other =>
            Err(Error::new(ErrorKind::InvalidData, format!("Expected Response, got {:?}", other))),
    }
}

pub async fn send_query(stream: &mut TcpStream) -> Result<Vec<Transaction>> {
    let msg = Message::Query;
    send_message(stream, &msg).await?;

    match network::receive_json::<Message>(stream).await? {
        Message::Response(txs) => Ok(txs), // basic ACK
        other =>
            Err(Error::new(ErrorKind::InvalidData, format!("Expected Response, got {:?}", other))),
    }
}

pub async fn send_ack(stream: &mut TcpStream) -> Result<()> {
    send_message(stream, &Message::Ack).await
}
