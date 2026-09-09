use serde::Deserialize;
use stellar_base::crypto::DalekKeyPair;
use stellar_base::network::Network;
use stellar_base::transaction::TransactionEnvelope;
use stellar_base::xdr::{XDRDeserialize, XDRSerialize};

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
        .json(&serde_json::json!({ "transaction": signed_xdr }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    validation.token
}

#[tokio::main]
async fn main() {
    // The anchor platform's own SEP server, not anchor-rust's business
    // server directly — SEP-10 and SEP-12's wire protocol are the
    // platform's job now; it calls back into our `/customer` handlers.
    let base_url = "http://localhost:8080";
    let http = reqwest::Client::new();
    let kp = DalekKeyPair::random().unwrap();
    let token = login(&http, base_url, &kp).await;
    println!("logged in as {}", kp.public_key().account_id());

    let get1 = http
        .get(format!("{base_url}/sep12/customer"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    println!("GET #1 status = {}", get1.status());
    println!("GET #1 body = {}", get1.text().await.unwrap());

    let put = http
        .put(format!("{base_url}/sep12/customer"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email_address": "ada@example.com"
        }))
        .send()
        .await
        .unwrap();
    println!("PUT status = {}", put.status());
    let put_body: serde_json::Value = put.json().await.unwrap();
    println!("PUT body = {put_body}");

    let get2 = http
        .get(format!("{base_url}/sep12/customer"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    println!("GET #2 status = {}", get2.status());
    println!("GET #2 body = {}", get2.text().await.unwrap());

    let id = put_body["id"].as_str().unwrap();
    let get_by_id = http
        .get(format!("{base_url}/sep12/customer?id={id}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    println!("GET by id status = {}", get_by_id.status());

    let account = kp.public_key().account_id();
    let delete = http
        .delete(format!("{base_url}/sep12/customer/{account}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    println!("DELETE status = {}", delete.status());

    let get3 = http
        .get(format!("{base_url}/sep12/customer"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    println!("GET #3 status (after delete) = {}", get3.status());
    println!("GET #3 body = {}", get3.text().await.unwrap());
}
