use superliquid::*;

use std::{ env, io::Error };

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let ip = "127.0.0.1:";
    let ports = ["6379", "6479", "6579", "6679"];
    let node_index = args
        .get(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    if node_index >= ports.len() {
        Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Invalid node index. Must be -1–{}.", ports.len() - 1)
        );
    }

    let addr = format!("{}{}", ip, ports[node_index]);
    let _ = match args.get(1).map(|s| s.as_str()) {
        Some("node") => { node::run_node(&addr).await }
        Some("client") => {
            let addr = format!("{}{}", ip, ports[0]); // Connect to node 0
            client::run_client(&addr).await
        }
        _ => {
            eprintln!("Usage: cargo run -- [node|client]");
            Ok(())
        }
    };
}
