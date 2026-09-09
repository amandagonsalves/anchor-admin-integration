use anchor_rust::ledger::Ledger;
use rust_decimal::Decimal;
use stellar_base::crypto::{DalekKeyPair, PublicKey};
use stellar_base::network::Network;
use stellar_base::operations::Operation;
use stellar_base::transaction::{MIN_BASE_FEE, Transaction};
use stellar_horizon::api::{accounts, transactions};
use stellar_horizon::client::{HorizonClient, HorizonHttpClient};

#[tokio::main]
async fn main() {
    let dest_kp = DalekKeyPair::random().unwrap();
    let dest_account = dest_kp.public_key().account_id();
    println!("destination = {dest_account}");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "https://friendbot.stellar.org/?addr={dest_account}"
        ))
        .send()
        .await
        .unwrap();
    println!("friendbot status = {}", resp.status());

    let distribution_seed = std::env::var("DISTRIBUTION_SEED").unwrap();
    let ledger = Ledger::new(
        "https://horizon-testnet.stellar.org",
        "Test SDF Network ; September 2015",
        &distribution_seed,
        "TEST".to_string(),
    )
    .unwrap();

    let asset = ledger.asset().unwrap();
    let destination = PublicKey::from_account_id(&dest_account).unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let horizon = HorizonHttpClient::new_from_str("https://horizon-testnet.stellar.org").unwrap();
    let network = Network::new_test();
    let (_, dest_resource) = horizon
        .request(accounts::single(&dest_kp.public_key()))
        .await
        .unwrap();
    let dest_sequence: i64 = dest_resource.sequence.parse().unwrap();

    let change_trust = Operation::new_change_trust()
        .with_asset(asset.clone().into())
        .with_limit(Some(stellar_base::amount::Stroops::max()))
        .unwrap()
        .build()
        .unwrap();
    let mut trust_tx = Transaction::builder(dest_kp.public_key(), dest_sequence + 1, MIN_BASE_FEE)
        .add_operation(change_trust)
        .into_transaction()
        .unwrap();
    trust_tx.sign(dest_kp.as_ref(), &network).unwrap();
    let trust_envelope = trust_tx.into_envelope();
    horizon
        .request(transactions::submit(&trust_envelope).unwrap())
        .await
        .unwrap();
    println!("trustline established");

    let hash = ledger
        .send_payment(destination.into(), asset, Decimal::new(100, 2), None)
        .await
        .unwrap();
    println!("payment tx hash = {hash}");
}
