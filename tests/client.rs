use superliquid::{
    message_protocol,
    node::{runner::run_node, state::PeerInfo},
    types::Transaction,
};

use std::{io::Result, sync::Arc};

use tokio::{
    net::TcpStream,
    sync::Mutex,
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

    let stream = TcpStream::connect("127.0.0.1:9000").await?;

    let (reader, writer) = stream.into_split();

    let reader = Arc::new(Mutex::new(reader));
    let writer = Arc::new(Mutex::new(writer));

    let txs_opt: Option<Vec<Transaction>> =
        message_protocol::send_query(reader.clone(), writer.clone()).await?;
    let txs = txs_opt.expect("Expect Some, got None");
    assert_eq!(txs.len(), 0);

    message_protocol::send_transaction(writer.clone(), tx.clone()).await?;

    let txs_opt: Option<Vec<Transaction>> =
        message_protocol::send_query(reader.clone(), writer.clone()).await?;
    let txs = txs_opt.expect("Expect Some, got None");
    assert_eq!(txs.len(), 1);

    message_protocol::send_end(writer).await?;
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

    let node_0_stream: TcpStream = TcpStream::connect("127.0.0.1:2001").await?;
    let node_1_stream: TcpStream = TcpStream::connect("127.0.0.1:2002").await?;

    let (reader_0, writer_0) = node_0_stream.into_split();
    let reader_0 = Arc::new(Mutex::new(reader_0));
    let writer_0 = Arc::new(Mutex::new(writer_0));

    let (_reader_1, writer_1) = node_1_stream.into_split();
    let writer_1 = Arc::new(Mutex::new(writer_1));

    let txs_opt: Option<Vec<Transaction>> =
        message_protocol::send_query(reader_0.clone(), writer_0.clone()).await?;
    let txs = txs_opt.expect("Expected Some, got none");
    assert_eq!(txs.len(), 0);

    message_protocol::send_transaction(writer_1.clone(), tx.clone()).await?;

    sleep(Duration::from_millis(100)).await;
    let txs_opt: Option<Vec<Transaction>> =
        message_protocol::send_query(reader_0, writer_0.clone()).await?;
    let txs = txs_opt.expect("Expected Some, got none");
    assert_eq!(txs.len(), 1);

    message_protocol::send_end(writer_0).await?;
    message_protocol::send_end(writer_1).await?;
    Ok(())
}
