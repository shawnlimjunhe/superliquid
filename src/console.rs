use colored::{self, Colorize};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hex::FromHex;
use rand::rngs::OsRng;
use std::{
    io::{self, Write},
    sync::Arc,
};
use tokio::{net::tcp::OwnedWriteHalf, sync::Mutex};

use crate::{
    client::ClientConnection,
    message_protocol::{self},
    types::transaction::{PublicKeyString, TransferTransaction, UnsignedTransaction},
};

pub struct ClientAccount {
    sk: SigningKey,
    pub(crate) _pk: VerifyingKey,
    pub(crate) pk_str: PublicKeyString,
}

impl ClientAccount {
    pub fn new(sk: SigningKey) -> Self {
        let pk = sk.verifying_key();
        let pk_str = PublicKeyString::from_public_key(&pk);
        let sk_hex: String = sk.to_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        println!("{:?}", sk_hex);
        Self {
            sk,
            _pk: pk,
            pk_str,
        }
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
        "  load <secret key>".blue(),
        "Loads an account from a secret key pair"
    );
    println!("{}   {}", "  drip".blue(), "Request balance from faucet");
    println!("{}", "  transfer <to> <amount>".blue());
    println!("{}", "  quit, q".blue());
}

fn handle_create() -> ClientAccount {
    let mut csprng = OsRng;

    let signing_key: SigningKey = SigningKey::generate(&mut csprng);

    let client_account = ClientAccount::new(signing_key);
    println!("Account created: {}", client_account.pk_str);
    client_account
}

async fn handle_drip(
    client: &Option<ClientAccount>,
    client_writer: Arc<Mutex<OwnedWriteHalf>>,
) -> std::io::Result<()> {
    let Some(client) = client else {
        println!("Please create or load an account before transferring.");
        return Ok(());
    };

    message_protocol::send_drip(client_writer, &client.pk_str.to_bytes()).await?;

    Ok(())
}

fn handle_load(trimmed: &str) -> ClientAccount {
    let sk_hex = trimmed["load ".len()..].trim();
    let sk_bytes = <[u8; 32]>::from_hex(sk_hex).expect("Invalid hex");

    let signing_key = SigningKey::from_bytes(&sk_bytes);
    let client_account = ClientAccount::new(signing_key);

    println!("Account loaded: {}", client_account.pk_str);
    client_account
}

async fn handle_transfer(
    trimmed: &str,
    client: &mut Option<ClientAccount>,
    client_connection: &ClientConnection,
) -> std::io::Result<()> {
    let Some(client) = client else {
        println!("Please create or load an account before transferring.");
        return Ok(());
    };

    let parts: Vec<&str> = trimmed["transfer ".len()..].split_whitespace().collect();
    if parts.len() != 2 {
        println!("Usage: transfer <to> <amount>");
    }

    // Ensure that pk is valid hex
    let to_pk_bytes = <[u8; 32]>::from_hex(parts[0]).expect("Invalid hex");
    let to_pk = VerifyingKey::from_bytes(&to_pk_bytes).expect("Invalid public key bytes");
    let to_pk = PublicKeyString::from_public_key(&to_pk);

    if to_pk == client.pk_str {
        println!("Can't transfer to self");
        return Ok(());
    }
    let amount = parts[1].parse::<u128>().expect("To be non negative number");

    let account_info = message_protocol::send_account_query(
        client.pk_str.to_bytes(),
        client_connection.reader.clone(),
        client_connection.writer.clone(),
    )
    .await?;

    if amount > account_info.balance {
        println!("Insufficient balance: {:?}", account_info.balance);
        return Ok(());
    }

    let txn = UnsignedTransaction::Transfer(TransferTransaction {
        from: client.pk_str.to_bytes(),
        to: to_pk.to_bytes(),
        amount,
        nonce: account_info.expected_nonce,
    });

    let tx = txn.sign(&mut client.sk);

    message_protocol::send_transaction(client_connection.writer.clone(), tx).await?;
    return Ok(());
}

async fn handle_query(
    client: &Option<ClientAccount>,
    client_connection: &ClientConnection,
) -> std::io::Result<()> {
    let Some(client) = client else {
        println!("Please create or load an account before transferring.");
        return Ok(());
    };

    let account_info = message_protocol::send_account_query(
        client.pk_str.to_bytes(),
        client_connection.reader.clone(),
        client_connection.writer.clone(),
    )
    .await?;

    println!(
        "Account: {:?}, balance: {:?}",
        client.pk_str, account_info.balance
    );

    return Ok(());
}

pub async fn run_console(addr: &str) -> std::io::Result<()> {
    const ANSI_ESC: &str = "\x1B[2J\x1B[1;1H";
    print!("{}", ANSI_ESC);
    println!("HotStuff Client Console");
    println!("Type `help` to see commands.");
    let connection = ClientConnection::create_client_connection(addr).await?;
    let mut client_account = None;
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
            "drip" => {
                handle_drip(&client_account, connection.writer.clone()).await?;
            }
            "query" => handle_query(&client_account, &connection).await?,
            _ if trimmed.starts_with("transfer ") => {
                handle_transfer(trimmed, &mut client_account, &connection).await?
            }
            "quit" | "q" => return Ok(()),
            _ => println!("Unknown command. Type `help` for options."),
        }
    }
}
