use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpStream};

use serde_json;

use crate::types::Transaction;

const LEN_BUF_LEN: usize = 4;

pub async fn send_message(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
  let data_len = (data.len() as u32).to_be_bytes();
  println!("Sending message of len: {:?}", data.len());
  let _ = stream.write_all(&data_len).await;
  let _ = stream.write_all(data).await;
  Ok(())
}

pub async fn receive_message(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
  println!("Receiving message...");
  let mut len_buf = [0u8; LEN_BUF_LEN];

  stream.read_exact(&mut len_buf).await?;

  let msg_len = u32::from_be_bytes(len_buf) as usize;

  println!("Expecting message of length: {}", msg_len);

  let mut resp_buf = vec![0u8; msg_len];

  stream.read_exact(&mut resp_buf).await?;
  
  println!("Received message");
  Ok(resp_buf)
}

pub async fn send_transaction(stream: &mut TcpStream, tx: &Transaction) -> std::io::Result<()> {
  let json = serde_json::to_vec(tx)?;
  send_message(stream, &json).await
}