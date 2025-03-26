use superliquid::{ node::run_node, types::Transaction, message_protocol };

use std::io::Result;

use tokio::{ net::TcpStream, time::{ sleep, Duration } };

#[tokio::test]
async fn test_transaction_round_trip() -> Result<()> {
    tokio::spawn(async {
        run_node("127.0.0.1:9000").await.unwrap();
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
