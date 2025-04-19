use superliquid::{
    client, config,
    node::{runner::run_node, state::PeerInfo},
};

use std::env;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let ip = "127.0.0.1";
    let base_port = 6400;
    let client_port = 8000;
    let num_nodes = config::retrieve_num_validators();

    let node_index = args
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    if node_index >= num_nodes {
        panic!("Invalid node index. Must be 0 to {}", num_nodes - 1);
    }

    let ports: Vec<u16> = (0..num_nodes).map(|i| base_port + (i as u16)).collect();
    let curr_port = ports[node_index];
    let consensus_addr = format!("{}:{}", ip, curr_port);
    let client_addr = format!("{}:{}", ip, client_port);

    let peers = ports
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != node_index)
        .map(|(i, port)| (i, format!("{}:{}", ip, port)))
        .map(|(i, addr)| PeerInfo {
            peer_id: i,
            peer_addr: addr,
        })
        .collect();

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    // 1) Create a secure RNG
    let mut csprng = OsRng;

    // 2) Generate a new random SigningKey
    let signing_key: SigningKey = SigningKey::generate(&mut csprng);

    // Get the 32‑byte secret key
    let sk_bytes = signing_key.to_bytes();
    // Convert to lowercase hex
    let sk_hex: String = sk_bytes.iter().map(|b| format!("{:02x}", b)).collect();

    // Derive and hex‑encode the public key
    let verifying_key = signing_key.verifying_key();
    let pk_bytes = verifying_key.to_bytes();
    let pk_hex: String = pk_bytes.iter().map(|b| format!("{:02x}", b)).collect();

    println!("SigningKey (hex):   {}", sk_hex);
    println!("VerifyingKey (hex): {}", pk_hex);

    let _ = match args.get(1).map(|s| s.as_str()) {
        Some("node") => run_node(client_addr, consensus_addr, peers, node_index).await,
        Some("client") => client::run_client(&client_addr).await,
        _ => {
            eprintln!("Usage: cargo run -- [node|client] [number]");
            Ok(())
        }
    };
}
