use superliquid::{ node::run_node, types::Transaction, message_protocol };

use std::io::Result;

use tokio::{ net::TcpStream, time::{ sleep, Duration } };

#[tokio::test]
async fn test_transaction_round_trip() -> Result<()> {
    tokio::spawn(async {
        run_node("127.0.0.1:9000", vec![]).await.unwrap();
    });

    // Give the node a moment to start
    sleep(Duration::from_millis(100)).await;

    // Run client logic
    let tx = Transaction {
        from: "alice".into(),
        to: "bob".into(),
        amount: 42,
    };

    let mut stream = TcpStream::connect("127.0.0.1:9000").await?;

    let txs: Vec<Transaction> = message_protocol::send_query(&mut stream).await?;
    assert_eq!(txs.len(), 0);

    message_protocol::send_transaction(&mut stream, tx.clone()).await?;

    let txs: Vec<Transaction> = message_protocol::send_query(&mut stream).await?;
    assert_eq!(txs.len(), 1);

    message_protocol::send_end(&mut stream).await?;
    Ok(())
}

#[tokio::test]
async fn test_transaction_broadcast() -> Result<()> {
    let node0 = tokio::spawn(async {
        run_node("127.0.0.1:9002", vec!["127.0.0.1:9001".to_string()]).await.unwrap();
    });

    let node1 = tokio::spawn(async {
        run_node("127.0.0.1:9001", vec!["127.0.0.1:9002".to_string()]).await.unwrap();
    });

    // Give the node a moment to start
    sleep(Duration::from_millis(500)).await;

    // Run client logic
    let tx = Transaction {
        from: "alice".into(),
        to: "bob".into(),
        amount: 42,
    };

    let mut node_0_stream: TcpStream = TcpStream::connect("127.0.0.1:9002").await?;
    let mut node_1_stream: TcpStream = TcpStream::connect("127.0.0.1:9001").await?;

    let txs: Vec<Transaction> = message_protocol::send_query(&mut node_1_stream).await?;
    assert_eq!(txs.len(), 0);

    message_protocol::send_transaction(&mut node_0_stream, tx.clone()).await?;

    let txs: Vec<Transaction> = message_protocol::send_query(&mut node_1_stream).await?;
    assert_eq!(txs.len(), 1);

    message_protocol::send_end(&mut node_0_stream).await?;
    message_protocol::send_end(&mut node_1_stream).await?;
    Ok(())
}
