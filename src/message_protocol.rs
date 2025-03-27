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

pub async fn send_transaction(stream: &mut TcpStream, tx: Transaction) -> Result<()> {
    let msg = Message::Transaction(tx);
    send_message(stream, &msg).await?;

    match network::receive_json::<Message, TcpStream>(stream).await? {
        Message::Ack => Ok(()), // basic ACK
        other =>
            Err(Error::new(ErrorKind::InvalidData, format!("Expected Response, got {:?}", other))),
    }
}

pub async fn send_query(stream: &mut TcpStream) -> Result<Vec<Transaction>> {
    let msg = Message::Query;
    send_message(stream, &msg).await?;

    match network::receive_json::<Message, TcpStream>(stream).await? {
        Message::Response(txs) => Ok(txs), // basic ACK
        other =>
            Err(Error::new(ErrorKind::InvalidData, format!("Expected Response, got {:?}", other))),
    }
}

pub async fn send_end(stream: &mut TcpStream) -> Result<()> {
    let msg = Message::End;
    send_message(stream, &msg).await
}

pub async fn send_ack(stream: &mut TcpStream) -> Result<()> {
    send_message(stream, &Message::Ack).await
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
                Message::Transaction(tx) => {
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
                Message::Query => {
                    let txs = vec![make_transaction()];
                    send_message(&mut socket, &Message::Response(txs)).await.unwrap();
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
