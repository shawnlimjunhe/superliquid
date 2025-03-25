use tokio::net::TcpStream;

use crate::message_protocol;
use crate::types::Transaction;

pub async fn run_client(addr: &str) -> std::io::Result<()> {
    let tx = Transaction {
        from: "alice".to_string(),
        to: "bob".to_string(),
        amount: 10,
    };

    let mut stream = TcpStream::connect(addr).await?;

    message_protocol::send_transaction(&mut stream, tx).await?;

    Ok(())
}
