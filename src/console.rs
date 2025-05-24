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
    state::{
        asset::{Asset, AssetId},
        spot_clearinghouse::AccountBalance,
    },
    types::transaction::{
        PublicKeyString, TransactionStatus, TransferTransaction, UnsignedTransaction,
    },
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

fn format_asset_balance(amount: u128, decimals: u8) -> String {
    let float = amount as f64 / 10f64.powi(decimals as i32);
    format!("{:.2}", float)
}

fn display_spot_balances(asset_infos: &Vec<Asset>, spot_balance: AccountBalance) {
    for token_balance in &spot_balance.asset_balances {
        let Some(asset) = asset_infos.get(token_balance.asset_id as usize) else {
            println!(
                "Could not find asset info for asset: {}",
                token_balance.asset_id
            );
            continue;
        };

        println!(
            "Asset: {}, Total: {}, Available: {}",
            asset.asset_name,
            format_asset_balance(token_balance.total_balance, asset.decimals),
            format_asset_balance(token_balance.available_balance, asset.decimals)
        );
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
    trimmed: &str,
    client: &Option<ClientAccount>,
    client_writer: Arc<Mutex<OwnedWriteHalf>>,
) -> std::io::Result<()> {
    let Some(client) = client else {
        println!("Please create or load an account before transferring.");
        return Ok(());
    };

    let parts: Vec<&str> = trimmed["drip ".len()..].split_whitespace().collect();
    println!("{:#?}", parts);
    if parts.len() != 1 {
        println!("Usage: drip <SUPE | USD>");
        return Ok(());
    }

    let asset_id = match parts[0].to_uppercase().as_str() {
        "SUPE" => 0,
        "USD" => 1,
        other => {
            println!("Unknown asset: {}", other);
            return Ok(());
        }
    };

    message_protocol::send_drip(client_writer, &client.pk_str.to_bytes(), asset_id).await?;

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
    if parts.len() != 3 {
        println!("Usage: transfer <to> <asset_id> <amount>");
    }

    // Ensure that pk is valid hex
    let to_pk_bytes = <[u8; 32]>::from_hex(parts[0]).expect("Invalid hex");
    let to_pk = VerifyingKey::from_bytes(&to_pk_bytes).expect("Invalid public key bytes");
    let to_pk = PublicKeyString::from_public_key(&to_pk);

    if to_pk == client.pk_str {
        println!("Can't transfer to self");
        return Ok(());
    }
    let asset_id = parts[1]
        .parse::<AssetId>()
        .expect("To be non negative number");

    let amount = parts[2].parse::<u128>().expect("To be non negative number");

    let account_info_with_balances = message_protocol::send_account_query(
        client.pk_str.to_bytes(),
        client_connection.reader.clone(),
        client_connection.writer.clone(),
    )
    .await?;

    let account_info = account_info_with_balances.account_info;
    let token_balances = account_info_with_balances.spot_balances.asset_balances;

    let token_balance_opt = token_balances
        .iter()
        .find(|asset| asset.asset_id == asset_id);

    let Some(token_balance) = token_balance_opt else {
        println!("Insufficient balance {:?}", 0);
        return Ok(());
    };

    if amount > token_balance.available_balance {
        println!(
            "Insufficient balance: Available balance {:?}",
            token_balance.available_balance
        );
        return Ok(());
    }

    let txn = UnsignedTransaction::Transfer(TransferTransaction {
        from: client.pk_str.to_bytes(),
        to: to_pk.to_bytes(),
        amount,
        asset_id,
        nonce: account_info.expected_nonce,
        status: TransactionStatus::Pending,
    });

    let tx = txn.sign(&mut client.sk);

    message_protocol::send_transaction(client_connection.writer.clone(), tx).await?;
    return Ok(());
}

async fn handle_query(
    client: &Option<ClientAccount>,
    client_connection: &ClientConnection,
    asset_infos: &Vec<Asset>,
) -> std::io::Result<()> {
    let Some(client) = client else {
        println!("Please create or load an account before transferring.");
        return Ok(());
    };

    let account_info_with_balances = message_protocol::send_account_query(
        client.pk_str.to_bytes(),
        client_connection.reader.clone(),
        client_connection.writer.clone(),
    )
    .await?;

    println!("Account: {:?}", client.pk_str,);
    display_spot_balances(asset_infos, account_info_with_balances.spot_balances);

    return Ok(());
}

async fn fetch_asset_infos(client_connection: &ClientConnection) -> std::io::Result<Vec<Asset>> {
    message_protocol::send_assets_query(
        client_connection.reader.clone(),
        client_connection.writer.clone(),
    )
    .await
}

pub async fn run_console(addr: &str) -> std::io::Result<()> {
    const ANSI_ESC: &str = "\x1B[2J\x1B[1;1H";
    print!("{}", ANSI_ESC);
    println!("HotStuff Client Console");
    println!("Type `help` to see commands.");
    let connection = ClientConnection::create_client_connection(addr).await?;
    let mut client_account = None;
    let asset_infos = fetch_asset_infos(&connection).await?;
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
            _ if trimmed.starts_with("drip") => {
                handle_drip(trimmed, &client_account, connection.writer.clone()).await?;
            }
            "query" => handle_query(&client_account, &connection, &asset_infos).await?,
            _ if trimmed.starts_with("transfer ") => {
                handle_transfer(trimmed, &mut client_account, &connection).await?
            }
            "quit" | "q" => return Ok(()),
            _ => println!("Unknown command. Type `help` for options."),
        }
    }
}
