use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}};

use crate::types::Transaction;

pub async fn run_node(addr: &str) -> std::io::Result<()> {
  // Bind the listener to the address
  let listener = TcpListener::bind(addr).await?;

  loop {
    let (socket, _) = listener.accept().await?;
    tokio::spawn(async move {
      process(socket).await;
    });
  }
}

async fn process(mut socket: TcpStream) {
  let mut len_buf = [0u8; 4];
  if socket.read_exact(&mut len_buf).await.is_err() {
    eprintln!("Failed to read length prefix");
    return;
  }
  
  let msg_len = u32::from_be_bytes(len_buf) as usize;

  let mut msg_buf = vec![0; msg_len];
  match socket.read_exact(&mut msg_buf).await {
    Ok(n) => {
      let raw = &msg_buf[..n];
      match serde_json::from_slice::<Transaction>(raw) {
        Ok(tx) => {
          println!("Recieved transaction: {:?}", tx);
          let resp = b"ok";
          let resp_length = (resp.len() as u32).to_be_bytes();
          let _ = socket.write_all(&resp_length).await;
          let _ = socket.write_all(resp).await;
        }
        Err(e) => {
          println!("Failed to parse json: {:?}", e);
          let resp = b"error";
          let resp_length = (resp.len() as u32).to_be_bytes();
          let _ = socket.write_all(&resp_length).await;
          let _ = socket.write_all(resp).await;
        }
      }
    }
    Err(e) => {
      eprintln!("Failed to read from socket: {}", e);
    }
  };
}
