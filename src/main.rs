use superliquid::*;

use std::env;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let ip = "127.0.0.1";
    let base_port = 6400;
    let client_port = 8000;
    let num_nodes = 4;

    let node_index = args
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    if node_index >= num_nodes {
        panic!("Invalid node index. Must be 0 to {}", num_nodes - 1);
    }

    let ports: Vec<u16> = (0..num_nodes).map(|i| base_port + (i as u16)).collect();
    let curr_port = ports[node_index];
    let peer_addr = format!("{}:{}", ip, curr_port);
    let client_addr = format!("{}:{}", ip, client_port);

    let peers = ports
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != node_index)
        .map(|(_, port)| format!("{}:{}", ip, port))
        .collect();

    println!("{}", peer_addr);
    let _ = match args.get(1).map(|s| s.as_str()) {
        Some("node") => { node::run_node(&client_addr, &peer_addr, peers, node_index).await }
        Some("client") => { client::run_client(&peer_addr).await }
        _ => {
            eprintln!("Usage: cargo run -- [node|client] [number]");
            Ok(())
        }
    };
}
