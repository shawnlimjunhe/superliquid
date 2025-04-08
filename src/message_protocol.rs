use serde::{ Deserialize, Serialize };
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::hotstuff::message::HotStuffMessage;
use crate::node::state::PeerId;
use crate::types::Transaction;
use crate::{ network, types::Message };
use std::io::{ Error, ErrorKind, Result };
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum AppMessage {
    Query,
    SubmitTransaction(Transaction),
    Response(Vec<Transaction>),
    Ack,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlMessage {
    Hello {
        peer_id: usize,
    },
    End, // Terminate connection
}

pub async fn receive_message(stream: &Arc<Mutex<TcpStream>>) -> Result<Option<Message>> {
    network::receive_json::<Message, TcpStream>(stream).await
}

pub async fn send_message(stream: &Arc<Mutex<TcpStream>>, message: &Message) -> Result<()> {
    let json = serde_json::to_vec(&message)?;
    let _ = network::send_data(stream, &json).await;
    Ok(())
}

pub async fn send_hotstuff_message(
    stream: &Arc<Mutex<TcpStream>>,
    message: &HotStuffMessage
) -> Result<()> {
    let json = serde_json::to_vec(&message)?;
    let _ = network::send_data(stream, &json).await;
    Ok(())
}

pub async fn send_hello(stream: Arc<Mutex<TcpStream>>, peer_id: PeerId) -> Result<()> {
    let msg = ControlMessage::Hello { peer_id };
    send_message(&stream, &&Message::Connection(msg)).await
}

pub async fn send_transaction(stream: &Arc<Mutex<TcpStream>>, tx: Transaction) -> Result<()> {
    let msg = AppMessage::SubmitTransaction(tx);
    send_message(stream, &Message::Application(msg)).await
}

pub async fn send_query(stream: &Arc<Mutex<TcpStream>>) -> Result<Option<Vec<Transaction>>> {
    let msg = AppMessage::Query;
    send_message(stream, &Message::Application(msg)).await?;

    Some(match receive_message(stream).await? {
        Some(Message::Application(AppMessage::Response(txs))) => Ok(Some(txs)),
        other =>
            Err(Error::new(ErrorKind::InvalidData, format!("Expected Response, got {:?}", other))),
    });
    return Ok(None);
}

pub async fn send_end(stream: &Arc<Mutex<TcpStream>>) -> Result<()> {
    let msg = ControlMessage::End;
    send_message(stream, &&Message::Connection(msg)).await
}

pub async fn send_ack(stream: &Arc<Mutex<TcpStream>>) -> Result<()> {
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
            let (socket, _) = listener.accept().await.unwrap();
            let socket = Arc::new(Mutex::new(socket));
            let msg = receive_message(&socket).await.unwrap();
            match msg {
                Some(Message::Application(AppMessage::SubmitTransaction(tx))) => {
                    assert_eq!(tx.amount, 42);
                }
                _ => panic!("Expected Transaction"),
            }
        });

        let stream = TcpStream::connect(addr).await?;
        let tx = make_transaction();
        let stream = Arc::new(Mutex::new(stream));
        send_transaction(&stream, tx).await
    }

    #[tokio::test]
    async fn test_send_and_receive_query_response() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?; // random port
        let addr = listener.local_addr()?;

        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let socket = Arc::new(Mutex::new(socket));
            let msg = receive_message(&socket).await.unwrap();
            match msg {
                Some(Message::Application(AppMessage::Query)) => {
                    let txs = vec![make_transaction()];
                    send_message(
                        &socket,
                        &&Message::Application(AppMessage::Response(txs))
                    ).await.unwrap();
                }
                _ => panic!("Expected Query"),
            }
        });

        let stream = TcpStream::connect(addr).await?;
        let stream = Arc::new(Mutex::new(stream));
        let txs = send_query(&stream).await?;
        let txs = txs.ok_or("Expected some, got none").unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].from, "alice");

        Ok(())
    }
}
