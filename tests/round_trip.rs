use superliquid::{
    message_protocol,
    node::{PeerInfo, run_node},
    types::Transaction,
};

use std::io::Result;

use tokio::{
    net::TcpStream,
    time::{Duration, sleep},
};

#[tokio::test]
async fn test_transaction_round_trip() -> Result<()> {
    tokio::spawn(async {
        run_node(
            "127.0.0.1:9000".to_string(),
            "127.to_string().0.0.1:8000".to_owned(),
            vec![],
            0,
        )
        .await
        .unwrap();
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
    let node_0_peer_info = PeerInfo {
        peer_id: 1,
        peer_addr: "127.0.0.1:3002".to_string(),
    };

    let node_1_peer_info = PeerInfo {
        peer_id: 0,
        peer_addr: "127.0.0.1:3001".to_string(),
    };

    let node_0_peers = vec![node_0_peer_info];
    let node_1_peers = vec![node_1_peer_info];

    tokio::spawn(async {
        run_node(
            "127.0.0.1:2001".to_string(),
            "127.0.0.1:3001".to_string(),
            node_0_peers,
            0,
        )
        .await
        .unwrap();
    });

    sleep(Duration::from_millis(50)).await;

    tokio::spawn(async {
        run_node(
            "127.0.0.1:2002".to_string(),
            "127.0.0.1:3002".to_string(),
            node_1_peers,
            1,
        )
        .await
        .unwrap();
    });

    // Give the node a moment to start
    sleep(Duration::from_millis(50)).await;

    // Run client logic
    let tx = Transaction {
        from: "alice".into(),
        to: "bob".into(),
        amount: 42,
    };

    let mut node_0_stream: TcpStream = TcpStream::connect("127.0.0.1:2001").await?;
    let mut node_1_stream: TcpStream = TcpStream::connect("127.0.0.1:2002").await?;

    let txs: Vec<Transaction> = message_protocol::send_query(&mut node_0_stream).await?;
    assert_eq!(txs.len(), 0);

    message_protocol::send_transaction(&mut node_1_stream, tx.clone()).await?;

    sleep(Duration::from_millis(100)).await;
    let txs: Vec<Transaction> = message_protocol::send_query(&mut node_0_stream).await?;
    assert_eq!(txs.len(), 1);

    message_protocol::send_end(&mut node_0_stream).await?;
    message_protocol::send_end(&mut node_1_stream).await?;
    Ok(())
}
