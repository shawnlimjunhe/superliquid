use tokio::net::{TcpListener, TcpStream};
use std::io::{Result, Error, ErrorKind};

use crate::{network, types::Transaction};
use crate::message_protocol;

struct Node {
  is_leader: bool,
  transactions: Vec<Transaction>,
  peers: Vec<Node>,
}

pub async fn run_node(addr: &str) -> Result<()> {
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

async fn process(mut socket: TcpStream) -> std::io::Result<()> {
  let transaction = message_protocol::listen_for_transaction(&mut socket).await?;
  println!("Received Transaction: {:?}", transaction);
  Ok(())
}

