use dotenv::dotenv;
use ed25519_dalek::{ SigningKey, VerifyingKey };
use hex::FromHex;
use std::collections::HashSet;
use std::env;
use std::time::Duration;

pub fn retrieve_signing_key(node_id: usize) -> SigningKey {
    dotenv().ok();
    let env_key = format!("SECRET_KEY_{}", node_id);
    let sk_hex = env::var(env_key).expect("SECRET_KEY not set");
    let sk_bytes = <[u8; 32]>::from_hex(&sk_hex).expect("Invalid hex");

    SigningKey::from_bytes(&sk_bytes)
}

pub fn retrieve_num_validators() -> usize {
    dotenv().ok();

    env::var("NUM_VALIDATORS")
        .expect("NUM_VALIDATORS not set")
        .parse::<usize>()
        .expect("NUM_VALIDATORS must be a number")
}

pub fn retrieve_validator_set() -> HashSet<VerifyingKey> {
    dotenv().ok();

    let num_validators = self::retrieve_num_validators();

    let mut validator_set = HashSet::new();
    for i in 0..num_validators {
        let env_key = format!("PUBLIC_KEY_{}", i);
        let pk_hex = env::var(&env_key).expect(&format!("{} not set", &env_key));

        let pk_bytes = <[u8; 32]>::from_hex(&pk_hex).expect("Invalid hex");
        let pk = VerifyingKey::from_bytes(&pk_bytes).expect("Invalid public key bytes");
        validator_set.insert(pk);
    }
    validator_set
}

pub fn retrieve_tick_duration() -> Duration {
    dotenv().ok();

    let duration_ms = env
        ::var("TICK_DURATION")
        .expect("TICK_DURATION not set")
        .parse::<u64>()
        .expect("TICK_DURATION must be a number");

    Duration::from_millis(duration_ms)
}
