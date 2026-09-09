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

#[derive(Deserialize, Debug)]
struct InteractiveTransactionResponse {
    url: String,
    id: String,
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

fn parse_url_params(url: &str) -> std::collections::HashMap<String, String> {
    let query = url.split('?').nth(1).unwrap_or("");
    query
        .split('&')
        .filter_map(|kv| {
            let mut parts = kv.splitn(2, '=');
            Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect()
}

#[tokio::main]
async fn main() {
    // The anchor platform's own SEP server. SEP-10/24's wire protocol is the
    // platform's job now; the interactive URL it hands back points at
    // anchor-rust's own hosted form (see `sep24::interactive`).
    let base_url = "http://localhost:8080";
    // anchor-rust's own business server, only ever hit directly for the
    // hosted interactive form's submit endpoint — every other call in this
    // test goes through the platform.
    let business_server_url = "http://localhost:8091";
    let http = reqwest::Client::new();

    let client_kp = DalekKeyPair::random().unwrap();
    let account = client_kp.public_key().account_id();
    println!("client account = {account}");

    http.get(format!("https://friendbot.stellar.org/?addr={account}"))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let horizon = HorizonHttpClient::new_from_str("https://horizon-testnet.stellar.org").unwrap();
    let network = Network::new_test();
    let distribution_account =
        PublicKey::from_account_id("GBLCQUFRITMMNXG6RNABCWXEEGRPT7Q36RKIVCFQ5ABUIYEOOI7RI2DH")
            .unwrap();
    let asset = Asset::new_credit("TEST", distribution_account).unwrap();

    // Establish trustline so the deposit can actually land.
    let (_, dest_resource) = horizon
        .request(accounts::single(&client_kp.public_key()))
        .await
        .unwrap();
    let dest_sequence: i64 = dest_resource.sequence.parse().unwrap();
    let change_trust = Operation::new_change_trust()
        .with_asset(asset.clone().into())
        .with_limit(Some(stellar_base::amount::Stroops::max()))
        .unwrap()
        .build()
        .unwrap();
    let mut trust_tx =
        Transaction::builder(client_kp.public_key(), dest_sequence + 1, MIN_BASE_FEE)
            .add_operation(change_trust)
            .into_transaction()
            .unwrap();
    trust_tx.sign(client_kp.as_ref(), &network).unwrap();
    horizon
        .request(transactions::submit(&trust_tx.into_envelope()).unwrap())
        .await
        .unwrap();
    println!("trustline established");

    let token = login(&http, base_url, &client_kp).await;
    println!("logged in");

    // ---- Deposit ----
    let deposit: InteractiveTransactionResponse = http
        .post(format!("{base_url}/sep24/transactions/deposit/interactive"))
        .bearer_auth(&token)
        .json(&json!({ "asset_code": "TEST" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("deposit created: id={}", deposit.id);

    let params = parse_url_params(&deposit.url);
    let form_resp = http.get(&deposit.url).send().await.unwrap();
    println!("interactive form status = {}", form_resp.status());

    let submit_resp = http
        .post(format!("{business_server_url}/sep24/interactive/submit"))
        .form(&[
            ("transaction_id", params["transaction_id"].as_str()),
            ("token", params["token"].as_str()),
            ("amount", "50"),
            ("first_name", "Ada"),
            ("last_name", "Lovelace"),
            ("email_address", "ada@example.com"),
        ])
        .send()
        .await
        .unwrap();
    println!("deposit submit status = {}", submit_resp.status());

    println!("waiting for deposit settlement...");
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    let deposit_status: serde_json::Value = http
        .get(format!("{base_url}/sep24/transaction?id={}", deposit.id))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("deposit final state = {deposit_status}");

    // ---- Withdrawal ----
    let withdraw: InteractiveTransactionResponse = http
        .post(format!(
            "{base_url}/sep24/transactions/withdraw/interactive"
        ))
        .bearer_auth(&token)
        .json(&json!({ "asset_code": "TEST" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("withdrawal created: id={}", withdraw.id);

    let wparams = parse_url_params(&withdraw.url);
    let wsubmit_resp = http
        .post(format!("{business_server_url}/sep24/interactive/submit"))
        .form(&[
            ("transaction_id", wparams["transaction_id"].as_str()),
            ("token", wparams["token"].as_str()),
            ("amount", "10"),
            ("first_name", "Ada"),
            ("last_name", "Lovelace"),
            ("email_address", "ada@example.com"),
        ])
        .send()
        .await
        .unwrap();
    let wsubmit_text = wsubmit_resp.text().await.unwrap();
    println!("withdraw submit body = {wsubmit_text}");

    let withdraw_status: serde_json::Value = http
        .get(format!("{base_url}/sep24/transaction?id={}", withdraw.id))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let withdraw_memo = withdraw_status["transaction"]["withdraw_memo"]
        .as_str()
        .unwrap()
        .to_string();
    println!("withdraw memo = {withdraw_memo}");

    // Actually send the funds to the distribution account with the assigned memo.
    let (_, client_resource) = horizon
        .request(accounts::single(&client_kp.public_key()))
        .await
        .unwrap();
    let client_sequence: i64 = client_resource.sequence.parse().unwrap();
    let distribution_account2 =
        PublicKey::from_account_id("GBLCQUFRITMMNXG6RNABCWXEEGRPT7Q36RKIVCFQ5ABUIYEOOI7RI2DH")
            .unwrap();
    let payment = Operation::new_payment()
        .with_destination(distribution_account2)
        .with_amount(stellar_base::amount::Amount::from_str("10").unwrap())
        .unwrap()
        .with_asset(asset)
        .build()
        .unwrap();
    let mut pay_tx =
        Transaction::builder(client_kp.public_key(), client_sequence + 1, MIN_BASE_FEE)
            .with_memo(stellar_base::memo::Memo::new_id(
                withdraw_memo.parse().unwrap(),
            ))
            .add_operation(payment)
            .into_transaction()
            .unwrap();
    pay_tx.sign(client_kp.as_ref(), &network).unwrap();
    horizon
        .request(transactions::submit(&pay_tx.into_envelope()).unwrap())
        .await
        .unwrap();
    println!("sent withdrawal payment to distribution account");

    println!("waiting for observer to pick up the payment...");
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;

    let final_withdraw_status: serde_json::Value = http
        .get(format!("{base_url}/sep24/transaction?id={}", withdraw.id))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("withdraw final state = {final_withdraw_status}");
}
