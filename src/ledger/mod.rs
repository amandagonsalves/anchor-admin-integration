use std::str::FromStr;

use rust_decimal::Decimal;
use stellar_base::asset::Asset;
use stellar_base::crypto::{DalekKeyPair, MuxedAccount, MuxedEd25519PublicKey, PublicKey};
use stellar_base::memo::Memo;
use stellar_base::network::Network;
use stellar_base::operations::Operation;
use stellar_base::transaction::{MIN_BASE_FEE, Transaction};
use stellar_horizon::api::{accounts, transactions};
use stellar_horizon::client::{HorizonClient, HorizonHttpClient};

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("stellar-base error: {0}")]
    Base(String),
    #[error("horizon error: {0}")]
    Horizon(String),
    #[error("invalid amount: {0}")]
    InvalidAmount(String),
}

pub type LedgerResult<T> = Result<T, LedgerError>;

pub struct Ledger {
    client: HorizonHttpClient,
    network: Network,
    distribution_keypair: DalekKeyPair,
    pub asset_code: String,
}

impl Ledger {
    pub fn new(
        horizon_url: &str,
        network_passphrase: &str,
        distribution_seed: &str,
        asset_code: String,
    ) -> LedgerResult<Self> {
        let client = HorizonHttpClient::new_from_str(horizon_url)
            .map_err(|e| LedgerError::Horizon(e.to_string()))?;
        let network = Network::new(network_passphrase.to_string());
        let distribution_keypair = DalekKeyPair::from_secret_seed(distribution_seed)
            .map_err(|e| LedgerError::Base(e.to_string()))?;

        Ok(Ledger {
            client,
            network,
            distribution_keypair,
            asset_code,
        })
    }

    pub fn horizon_client(&self) -> &HorizonHttpClient {
        &self.client
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn distribution_public_key(&self) -> PublicKey {
        self.distribution_keypair.public_key()
    }

    pub fn distribution_account_id(&self) -> String {
        self.distribution_public_key().account_id()
    }

    pub fn asset(&self) -> LedgerResult<Asset> {
        Asset::new_credit(self.asset_code.clone(), self.distribution_public_key())
            .map_err(|e| LedgerError::Base(e.to_string()))
    }

    /// Fetches an account's current sequence number from Horizon.
    pub async fn load_sequence(&self, account: &PublicKey) -> LedgerResult<i64> {
        let (_, resource) = self
            .client
            .request(accounts::single(account))
            .await
            .map_err(|e| LedgerError::Horizon(e.to_string()))?;
        resource
            .sequence
            .parse()
            .map_err(|_| LedgerError::Horizon("invalid sequence number from horizon".into()))
    }

    /// Parses a Stellar strkey account id (`G...` or `M...`) into a [`MuxedAccount`]
    /// suitable for use as a payment destination.
    pub fn parse_account(account_id: &str) -> LedgerResult<MuxedAccount> {
        if account_id.starts_with('M') {
            MuxedEd25519PublicKey::from_account_id(account_id)
                .map(Into::into)
                .map_err(|e| LedgerError::Base(format!("invalid muxed account: {e}")))
        } else {
            PublicKey::from_account_id(account_id)
                .map(Into::into)
                .map_err(|e| LedgerError::Base(format!("invalid account: {e}")))
        }
    }

    /// Sends a payment of `asset` from the distribution account to `destination`,
    /// optionally attaching `memo`. Returns the submitted transaction's hash (hex).
    pub async fn send_payment(
        &self,
        destination: MuxedAccount,
        asset: Asset,
        amount: Decimal,
        memo: Option<Memo>,
    ) -> LedgerResult<String> {
        let source_public_key = self.distribution_public_key();
        let sequence = self.load_sequence(&source_public_key).await?;

        let stellar_amount = stellar_base::amount::Amount::from_str(&amount.to_string())
            .map_err(|_| LedgerError::InvalidAmount(amount.to_string()))?;

        let payment = Operation::new_payment()
            .with_destination(destination)
            .with_amount(stellar_amount)
            .map_err(|e| LedgerError::Base(e.to_string()))?
            .with_asset(asset)
            .build()
            .map_err(|e| LedgerError::Base(e.to_string()))?;

        let mut builder = Transaction::builder(source_public_key, sequence + 1, MIN_BASE_FEE)
            .add_operation(payment);
        if let Some(memo) = memo {
            builder = builder.with_memo(memo);
        }

        let mut tx = builder
            .into_transaction()
            .map_err(|e| LedgerError::Base(e.to_string()))?;
        tx.sign(self.distribution_keypair.as_ref(), &self.network)
            .map_err(|e| LedgerError::Base(e.to_string()))?;

        let hash = hex::encode(
            tx.hash(&self.network)
                .map_err(|e| LedgerError::Base(e.to_string()))?,
        );

        let envelope = tx.into_envelope();
        let submit_request =
            transactions::submit(&envelope).map_err(|e| LedgerError::Base(e.to_string()))?;
        self.client
            .request(submit_request)
            .await
            .map_err(|e| LedgerError::Horizon(e.to_string()))?;

        Ok(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_account_accepts_g_address() {
        let account =
            Ledger::parse_account("GBLGJA4TUN5XOGTV6WO2BWYUI2OZR5GYQ5PDPCRMQ5XEPJOYWB2X4CJO")
                .unwrap();
        assert!(matches!(account, MuxedAccount::Ed25519(_)));
    }

    #[test]
    fn parse_account_accepts_m_address() {
        let account = Ledger::parse_account(
            "MBFZNZTFSI6TWLVAID7VOLCIFX2PMUOS2X7U6H4TNK4PAPSHPWMMUAAAAAAAAAPCIA2IM",
        )
        .unwrap();
        assert!(matches!(account, MuxedAccount::MuxedEd25519(_)));
    }

    #[test]
    fn parse_account_rejects_garbage() {
        assert!(Ledger::parse_account("not-an-account").is_err());
    }
}
