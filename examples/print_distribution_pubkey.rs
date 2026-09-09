//! Prints the public key for the seed in `DISTRIBUTION_SEED`. Used by
//! `local/anchor-platform/ap_start.sh` to template `assets.yaml` /
//! `stellar.localhost.toml` without hardcoding the distribution account
//! anywhere outside `.env`.

use stellar_base::crypto::DalekKeyPair;

fn main() {
    let seed = std::env::var("DISTRIBUTION_SEED").expect("DISTRIBUTION_SEED must be set");
    let keypair = DalekKeyPair::from_secret_seed(&seed).expect("invalid DISTRIBUTION_SEED");
    println!("{}", keypair.public_key().account_id());
}
