use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpStream};

use serde_json;

use crate::types::Transaction;

pub async fn run_client(addr: &str) -> std::io::Result<()> {

  let tx = Transaction {
    from: "alice".to_string(),
    to: "bob".to_string(),
    amount: 10,
  };

  let json = serde_json::to_vec(&tx).unwrap();

  let length = (json.len() as u32).to_be_bytes();


  let mut stream = TcpStream::connect(addr).await?;

  let _ = stream.write_all(&length).await;
  let _ = stream.write_all(&json).await;

  let mut len_buf = [0u8; 4];

  stream.read_exact(&mut len_buf).await?;

  let msg_len = u32::from_be_bytes(len_buf) as usize;

  let mut resp_buf = vec![0u8; msg_len];
  stream.read_exact(&mut resp_buf).await?;

  let response = String::from_utf8_lossy(&resp_buf);
  println!("Recieved resp from server: {:?}", response);

  Ok(())
}