mod types;
mod node;
mod client;
mod message_protocol;
mod network;

use std::env;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let addr = "127.0.0.1:6379";

    let _ = match args.get(1).map(|s| s.as_str()) {
        Some("node") => node::run_node(addr).await,
        Some("client") => client::run_client(addr).await,
        _ => {
            eprintln!("Usage: cargo run -- [node|client]");
            Ok(())
        }
    };
}
