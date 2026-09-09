use std::str::FromStr;

use serde::Deserialize;
use serde_json::json;
use stellar_base::asset::Asset;
use stellar_base::crypto::{DalekKeyPair, PublicKey};
use stellar_base::network::Network;
use stellar_base::operations::Operation;
use stellar_base::transaction::{MIN_BASE_FEE, Transaction, TransactionEnvelope};
use stellar_base::xdr::{XDRDeserialize, XDRSerialize};
use stellar_horizon::api::{accounts, transactions};
use stellar_horizon::client::{HorizonClient, HorizonHttpClient};

#[derive(Deserialize)]
struct ChallengeResponse {
    transaction: String,
    network_passphrase: String,
}
#[derive(Deserialize)]
struct ValidationResponse {
    token: String,
}

async fn login(http: &reqwest::Client, base_url: &str, kp: &DalekKeyPair) -> String {
    let account = kp.public_key().account_id();
    let challenge: ChallengeResponse = http
        .get(format!("{base_url}/auth?account={account}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let envelope = TransactionEnvelope::from_xdr_base64(&challenge.transaction).unwrap();
    let mut tx = envelope.as_transaction().unwrap().clone();
    let network = Network::new(challenge.network_passphrase.clone());
    tx.sign(kp.as_ref(), &network).unwrap();
    let signed_xdr = tx.into_envelope().xdr_base64().unwrap();
    let validation: ValidationResponse = http
        .post(format!("{base_url}/auth"))
        .json(&json!({ "transaction": signed_xdr }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    validation.token
}

async fn send_with_memo(
    horizon: &HorizonHttpClient,
    network: &Network,
    sender_kp: &DalekKeyPair,
    distribution: &PublicKey,
    asset: &Asset,
    amount: &str,
    memo: u64,
) {
    let (_, resource) = horizon
        .request(accounts::single(&sender_kp.public_key()))
        .await
        .unwrap();
    let sequence: i64 = resource.sequence.parse().unwrap();
    let payment = Operation::new_payment()
        .with_destination(*distribution)
        .with_amount(stellar_base::amount::Amount::from_str(amount).unwrap())
        .unwrap()
        .with_asset(asset.clone())
        .build()
        .unwrap();
    let mut tx = Transaction::builder(sender_kp.public_key(), sequence + 1, MIN_BASE_FEE)
        .with_memo(stellar_base::memo::Memo::new_id(memo))
        .add_operation(payment)
        .into_transaction()
        .unwrap();
    tx.sign(sender_kp.as_ref(), network).unwrap();
    horizon
        .request(transactions::submit(&tx.into_envelope()).unwrap())
        .await
        .unwrap();
}

#[tokio::main]
async fn main() {
    // The anchor platform's own SEP server (SEP-10/12/24/31 wire protocol).
    let base_url = "http://localhost:8080";
    // anchor-rust's own business server, hit directly only for the hosted
    // interactive form's submit endpoint (part of the self-deposit setup
    // below, unrelated to SEP-31 itself).
    let business_server_url = "http://localhost:8091";
    let http = reqwest::Client::new();
    let horizon = HorizonHttpClient::new_from_str("https://horizon-testnet.stellar.org").unwrap();
    let network = Network::new_test();
    let distribution =
        PublicKey::from_account_id("GBLCQUFRITMMNXG6RNABCWXEEGRPT7Q36RKIVCFQ5ABUIYEOOI7RI2DH")
            .unwrap();
    let asset = Asset::new_credit("TEST", distribution).unwrap();

    // "Sending anchor" identity: has a TEST balance already (from a prior deposit)
    // and holds no trustline requirement since it's the one paying out, not
    // receiving on-chain here (it already holds TEST from smoke_sep24's deposit
    // run, or we fund + trustline + deposit-to-self fresh so this test is
    // self-contained).
    let sender_kp = DalekKeyPair::random().unwrap();
    let sender_account = sender_kp.public_key().account_id();
    http.get(format!(
        "https://friendbot.stellar.org/?addr={sender_account}"
    ))
    .send()
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let (_, resource) = horizon
        .request(accounts::single(&sender_kp.public_key()))
        .await
        .unwrap();
    let sequence: i64 = resource.sequence.parse().unwrap();
    let change_trust = Operation::new_change_trust()
        .with_asset(asset.clone().into())
        .with_limit(Some(stellar_base::amount::Stroops::max()))
        .unwrap()
        .build()
        .unwrap();
    let mut trust_tx = Transaction::builder(sender_kp.public_key(), sequence + 1, MIN_BASE_FEE)
        .add_operation(change_trust)
        .into_transaction()
        .unwrap();
    trust_tx.sign(sender_kp.as_ref(), &network).unwrap();
    horizon
        .request(transactions::submit(&trust_tx.into_envelope()).unwrap())
        .await
        .unwrap();

    let token = login(&http, base_url, &sender_kp).await;

    // Fund the sender with TEST via a self-deposit so it has something to pay a SEP-31 transaction with.
    let deposit: serde_json::Value = http
        .post(format!("{base_url}/sep24/transactions/deposit/interactive"))
        .bearer_auth(&token)
        .json(&json!({ "asset_code": "TEST" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let url = deposit["url"].as_str().unwrap();
    let params: std::collections::HashMap<String, String> = url
        .split('?')
        .nth(1)
        .unwrap()
        .split('&')
        .filter_map(|kv| {
            let mut p = kv.splitn(2, '=');
            Some((p.next()?.to_string(), p.next()?.to_string()))
        })
        .collect();
    http.post(format!("{business_server_url}/sep24/interactive/submit"))
        .form(&[
            ("transaction_id", params["transaction_id"].as_str()),
            ("token", params["token"].as_str()),
            ("amount", "100"),
            ("first_name", "Sender"),
            ("last_name", "Anchor"),
            ("email_address", "sender@example.com"),
        ])
        .send()
        .await
        .unwrap();
    println!("self-deposit submitted, waiting for settlement...");
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    // ---- Case A: SEP-31 transaction with no receiver_id -> settles immediately ----
    let tx_a: serde_json::Value = http
        .post(format!("{base_url}/sep31/transactions"))
        .bearer_auth(&token)
        .json(&json!({ "amount": "20", "asset_code": "TEST" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("case A created: {tx_a}");
    let memo_a: u64 = tx_a["stellar_memo"].as_str().unwrap().parse().unwrap();
    send_with_memo(
        &horizon,
        &network,
        &sender_kp,
        &distribution,
        &asset,
        "20",
        memo_a,
    )
    .await;
    println!("case A payment sent, waiting for observer...");
    tokio::time::sleep(std::time::Duration::from_secs(7)).await;
    let status_a: serde_json::Value = http
        .get(format!(
            "{base_url}/sep31/transactions/{}",
            tx_a["id"].as_str().unwrap()
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("case A final: {status_a}");

    // ---- Case B: SEP-31 transaction WITH receiver_id, incomplete KYC -> pending_customer_info_update, then resumes after SEP-12 PUT ----
    let receiver_customer: serde_json::Value = http
        .put(format!("{base_url}/sep12/customer"))
        .bearer_auth(&token)
        .json(&json!({ "type": "sep31-receiver" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let receiver_id = receiver_customer["id"].as_str().unwrap().to_string();
    println!("receiver customer id = {receiver_id}");

    let tx_b: serde_json::Value = http
        .post(format!("{base_url}/sep31/transactions"))
        .bearer_auth(&token)
        .json(&json!({ "amount": "15", "asset_code": "TEST", "receiver_id": receiver_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("case B created: {tx_b}");
    let memo_b: u64 = tx_b["stellar_memo"].as_str().unwrap().parse().unwrap();
    send_with_memo(
        &horizon,
        &network,
        &sender_kp,
        &distribution,
        &asset,
        "15",
        memo_b,
    )
    .await;
    println!("case B payment sent, waiting for observer...");
    tokio::time::sleep(std::time::Duration::from_secs(7)).await;
    let status_b1: serde_json::Value = http
        .get(format!(
            "{base_url}/sep31/transactions/{}",
            tx_b["id"].as_str().unwrap()
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("case B after payment (expect pending_customer_info_update): {status_b1}");

    // Now complete the receiver's KYC -> should auto-resume via the SEP-12 hook.
    http.put(format!("{base_url}/sep12/customer"))
        .bearer_auth(&token)
        .json(&json!({
            "type": "sep31-receiver",
            "bank_account_number": "0001112223",
            "bank_account_type": "checking",
            "bank_number": "021000021",
            "bank_branch_number": "001"
        }))
        .send()
        .await
        .unwrap();
    println!("receiver KYC completed, waiting for resume...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let status_b2: serde_json::Value = http
        .get(format!(
            "{base_url}/sep31/transactions/{}",
            tx_b["id"].as_str().unwrap()
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("case B after KYC completed (expect completed): {status_b2}");
}
