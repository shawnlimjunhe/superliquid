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

pub async fn recieve_message(stream: &mut TcpStream) -> Result<Message> {
    network::receive_json::<Message>(stream).await
}

async fn send_message(stream: &mut TcpStream, message: &Message) -> Result<()> {
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

pub async fn listen_for_transaction(stream: &mut TcpStream) -> Result<Transaction> {
    let message = recieve_message(stream).await?;

    match message {
        Message::Transaction(tx) => {
            send_message(stream, &Message::Ack).await?;
            Ok(tx)
        }
        other =>
            Err(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Expected Query, got {:?}", other)
                )
            ),
    }
}
