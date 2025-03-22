use tokio::net::{TcpListener, TcpStream};

use crate::{network::receive_message, types::Transaction};
use crate::network;

pub async fn run_node(addr: &str) -> std::io::Result<()> {
  // Bind the listener to the address
  let listener = TcpListener::bind(addr).await?;

  println!("Listening on addr: {:?}", addr);
  loop {
    let (socket, _) = listener.accept().await?;
    tokio::spawn(async move {
       match process(socket).await {
        Ok(()) => println!("Success"),
        Err(e) => println!("Failed due to: {:?}", e),
       }
    });
  }
}

async fn process(mut socket: TcpStream) -> std::io::Result<()>{

  let resp_buff = receive_message(&mut socket).await?;

  match serde_json::from_slice::<Transaction>(&resp_buff) {
    Ok(tx) => {
      println!("Recieved transaction: {:?}", tx);
      let resp = b"ok";
      network::send_message(&mut socket, resp).await?;
    }
    Err(e) => {
      println!("Failed to parse json: {:?}", e);
      let resp = b"error";
      network::send_message(&mut socket, resp).await?;
    }
  };
  
  Ok(())
}
