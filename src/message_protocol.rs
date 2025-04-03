use serde::{ Deserialize, Serialize };
use tokio::net::TcpStream;

use crate::hotstuff::message::HotStuffMessage;
use crate::types::Transaction;
use crate::{ network, types::Message };
use std::io::{ Error, ErrorKind, Result };

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum AppMessage {
    Query,
    SubmitTransaction(Transaction),
    Response(Vec<Transaction>),
    Ack,
    End, // Terminate connection
}

pub async fn receive_message(stream: &mut TcpStream) -> Result<Message> {
    network::receive_json::<Message, TcpStream>(stream).await
}

pub async fn send_message(stream: &mut TcpStream, message: &Message) -> Result<()> {
    let json = serde_json::to_vec(&message)?;
    let _ = network::send_data(stream, &json).await;
    Ok(())
}

pub async fn send_hotstuff_message(
    stream: &mut TcpStream,
    message: &HotStuffMessage
) -> Result<()> {
    let json = serde_json::to_vec(&message)?;
    let _ = network::send_data(stream, &json).await;
    Ok(())
}

pub async fn send_transaction(stream: &mut TcpStream, tx: Transaction) -> Result<()> {
    let msg = AppMessage::SubmitTransaction(tx);
    send_message(stream, &Message::Application(msg)).await?;

    match receive_message(stream).await? {
        Message::Application(AppMessage::Ack) => Ok(()), // basic ACK
        other =>
            Err(Error::new(ErrorKind::InvalidData, format!("Expected Response, got {:?}", other))),
    }
}

pub async fn send_query(stream: &mut TcpStream) -> Result<Vec<Transaction>> {
    let msg = AppMessage::Query;
    send_message(stream, &Message::Application(msg)).await?;

    match receive_message(stream).await? {
        Message::Application(AppMessage::Response(txs)) => Ok(txs), // basic ACK
        other =>
            Err(Error::new(ErrorKind::InvalidData, format!("Expected Response, got {:?}", other))),
    }
}

pub async fn send_end(stream: &mut TcpStream) -> Result<()> {
    let msg = AppMessage::End;
    send_message(stream, &Message::Application(msg)).await
}

pub async fn send_ack(stream: &mut TcpStream) -> Result<()> {
    let msg = AppMessage::Ack;
    send_message(stream, &Message::Application(msg)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{ TcpListener, TcpStream };

    fn make_transaction() -> Transaction {
        Transaction {
            from: "alice".into(),
            to: "bob".into(),
            amount: 42,
        }
    }

    #[tokio::test]
    async fn test_send_and_receive_transaction() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?; // random port
        let addr = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let msg = receive_message(&mut socket).await.unwrap();
            match msg {
                Message::Application(AppMessage::SubmitTransaction(tx)) => {
                    assert_eq!(tx.amount, 42);
                    send_ack(&mut socket).await.unwrap();
                }
                _ => panic!("Expected Transaction"),
            }
        });

        let mut stream = TcpStream::connect(addr).await?;
        let tx = make_transaction();
        send_transaction(&mut stream, tx).await
    }

    #[tokio::test]
    async fn test_send_and_receive_query_response() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?; // random port
        let addr = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let msg = receive_message(&mut socket).await.unwrap();
            match msg {
                Message::Application(AppMessage::Query) => {
                    let txs = vec![make_transaction()];
                    send_message(
                        &mut socket,
                        &&Message::Application(AppMessage::Response(txs))
                    ).await.unwrap();
                }
                _ => panic!("Expected Query"),
            }
        });

        let mut stream = TcpStream::connect(addr).await?;
        let txs = send_query(&mut stream).await?;
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].from, "alice");
        Ok(())
    }
}
