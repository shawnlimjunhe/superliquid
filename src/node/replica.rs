use std::io::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::types::ReplicaOutbound;

use super::peer::broadcast::{broadcast_hotstuff_message, send_to_peer};
use super::state::Node;

pub(super) async fn handle_replica_outbound(
    mut from_replica_rx: mpsc::Receiver<ReplicaOutbound>,
    node: Arc<Node>,
) -> Result<()> {
    while let Some(outbound_msg) = from_replica_rx.recv().await {
        match outbound_msg {
            ReplicaOutbound::Broadcast(msg) => broadcast_hotstuff_message(&node, msg).await?,
            ReplicaOutbound::SendTo(peer_id, msg) => {
                send_to_peer(&node, msg, peer_id).await?;
            }
        }
    }
    Ok(())
}
