use std::io::Result;
use std::sync::Arc;

use futures::future::join_all;
use tokio::{net::TcpStream, sync::Mutex};

use crate::{
    hotstuff::message::HotStuffMessage,
    message_protocol::{send_message, send_transaction},
    node::state::{Node, PeerId},
    types::{Message, Transaction},
};

/// Broadcast msg to all peer connections

pub(crate) async fn broadcast_hotstuff_message(
    node: &Arc<Node>,
    msg: HotStuffMessage,
) -> Result<()> {
    let peer_connections: Vec<Arc<Mutex<TcpStream>>> = {
        let peer_connections = node.peer_connections.lock().await;
        peer_connections.values().cloned().collect()
    };

    for stream in peer_connections {
        let cloned_msg = msg.clone();
        tokio::spawn(async move {
            let mut stream = stream.lock().await;
            send_message(&mut stream, &Message::HotStuff(cloned_msg)).await
        });
    }

    Ok(())
}

pub(crate) async fn send_to_peer(
    node: &Arc<Node>,
    msg: HotStuffMessage,
    peer_id: PeerId,
) -> Result<()> {
    let peer_connection = {
        let peer_connections = node.peer_connections.lock().await;
        peer_connections.get(&peer_id).cloned()
    };

    let Some(peer_connection) = peer_connection else {
        return Ok(());
    };
    {
        let mut peer_connection = peer_connection.lock().await;
        send_message(&mut peer_connection, &Message::HotStuff(msg)).await
    }
}

pub(crate) async fn broadcast_transaction(node: &Arc<Node>, tx: Transaction) -> Result<()> {
    let id = node.id;

    let peer_connections: Vec<Arc<Mutex<TcpStream>>> = {
        let peer_connections = node.peer_connections.lock().await;
        peer_connections.values().cloned().collect()
    };

    let mut tasks = Vec::new();

    println!(
        "broadcasting tx from node {} to {} peers",
        id,
        peer_connections.len()
    );

    for stream in peer_connections {
        let cloned_tx = tx.clone();
        let task = tokio::spawn(async move {
            let mut stream = stream.lock().await;
            send_transaction(&mut stream, cloned_tx).await
        });
        tasks.push(task);
    }

    let results = join_all(tasks).await;
    for result in results {
        match result {
            Ok(Ok(())) => {
                println!("sent transaction ");
            }
            Ok(Err(e)) => eprintln!("send_transaction error: {:?}", e),
            Err(e) => eprintln!("task panicked: {:?}", e),
        }
    }

    println!("Finish broadcasting tx");
    Ok(())
}
