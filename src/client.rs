use tokio::net::TcpStream;

use crate::types::Transaction;
use crate::network;

pub async fn run_client(addr: &str) -> std::io::Result<()> {

  let tx = Transaction {
    from: "alice".to_string(),
    to: "bob".to_string(),
    amount: 10,
  };

  let mut stream = TcpStream::connect(addr).await?;

  let _  = network::send_transaction(&mut stream, &tx).await?;
  println!("Send tx to server");

  let resp_buf = network::receive_message(&mut stream).await?;
  let response = String::from_utf8_lossy(&resp_buf);
  println!("Recieved resp from server: {:?}", response);

  Ok(())
}