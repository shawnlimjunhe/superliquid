use colored::{self, Colorize};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hex::FromHex;
use rand::rngs::OsRng;
use std::io::{self, Write};

use crate::{
    client::ClientConnection,
    message_protocol::{self},
};

pub struct ClientAccount {
    sk: SigningKey,
    pub(crate) pk: VerifyingKey,
    pub(crate) pk_hex: String,
}

impl ClientAccount {
    pub fn new(sk: SigningKey) -> Self {
        let pk = sk.verifying_key();
        let pk_bytes = pk.to_bytes();
        let pk_hex: String = pk_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let sk_hex: String = sk.to_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        println!("{:?}", sk_hex);
        Self { sk, pk, pk_hex }
    }
}

fn handle_help() {
    println!("{}", "Commands:".green());
    println!(
        "{}   {}",
        "  create, c".blue(),
        "Creates a new public key secret key pair"
    );
    println!(
        "{}   {}",
        "  load, l <secret key>".blue(),
        "Loads an account from a secret key pair"
    );
    println!("{}   {}", "  drip, d".blue(), "Request balance from faucet");
    println!("{}", "  transfer <from> <to> <amount>".blue());
    println!("{}", "  quit, q".blue());
}

fn handle_create() -> ClientAccount {
    let mut csprng = OsRng;

    let signing_key: SigningKey = SigningKey::generate(&mut csprng);

    let client_account = ClientAccount::new(signing_key);
    println!("Account created: {}", client_account.pk_hex);
    client_account
}

async fn handle_drip(
    client: &Option<ClientAccount>,
    connection: ClientConnection,
) -> std::io::Result<()> {
    let Some(client) = client else {
        println!("Please create or load an account before transferring.");
        return Ok(());
    };

    message_protocol::send_drip(connection.writer, &client.pk_hex).await?;

    Ok(())
}

fn handle_load(trimmed: &str) -> ClientAccount {
    let sk_hex = trimmed["load ".len()..].trim();
    let sk_bytes = <[u8; 32]>::from_hex(sk_hex).expect("Invalid hex");

    let signing_key = SigningKey::from_bytes(&sk_bytes);
    let client_account = ClientAccount::new(signing_key);
    println!("Account loaded: {}", client_account.pk_hex);
    client_account
}

fn handle_transfer(trimmed: &str, client: &Option<ClientAccount>) {
    let Some(client) = client else {
        println!("Please create or load an account before transferring.");
        return;
    };

    let parts: Vec<&str> = trimmed["transfer ".len()..].split_whitespace().collect();
    if parts.len() == 3 {
        println!("{:?}", parts);
    } else {
        println!("Usage: transfer <from> <to> <amount>");
    }
}

pub async fn run_console() -> std::io::Result<()> {
    const ANSI_ESC: &str = "\x1B[2J\x1B[1;1H";
    print!("{}", ANSI_ESC);
    println!("HotStuff Client Console");
    println!("Type `help` to see commands.");
    let mut client_account: Option<ClientAccount> = None;
    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let trimmed = input.trim();

        match trimmed {
            "help" => handle_help(),
            "create" | "c" => {
                client_account = Some(handle_create());
            }
            _ if trimmed.starts_with("load ") => client_account = Some(handle_load(trimmed)),
            _ if trimmed.starts_with("transfer ") => handle_transfer(trimmed, &client_account),
            "quit" | "q" => return Ok(()),
            _ => println!("Unknown command. Type `help` for options."),
        }
    }
}
