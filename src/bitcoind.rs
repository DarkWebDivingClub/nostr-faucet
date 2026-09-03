//! Minimal JSON-RPC to the miner's `bitcoind`.
//!
//! Deliberately small. This connection can spend a wallet holding tens of
//! thousands of coins, so it does exactly four things and nothing else.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::config::BitcoindConfig;

pub struct Bitcoind {
    url: String,
    user: String,
    password: String,
    client: reqwest::Client,
    pub chain_label: String,
}

/// What the faucet can say about its own wallet, for `get_info` and for
/// refusing clearly when there is nothing to pay with.
pub struct WalletStatus {
    pub spendable_sat: u64,
    pub chain: String,
    pub block_height: u64,
    pub block_hash: String,
}

impl Bitcoind {
    pub fn new(cfg: &BitcoindConfig) -> Self {
        Self {
            url: format!(
                "http://{}:{}/wallet/{}",
                cfg.rpc_host, cfg.rpc_port, cfg.wallet
            ),
            user: cfg.rpc_user.clone(),
            password: cfg.rpc_password.clone(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            chain_label: cfg.chain_label.clone(),
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({
            "jsonrpc": "1.0",
            "id": "nostr-faucet",
            "method": method,
            "params": params,
        });

        let resp = self
            .client
            .post(&self.url)
            .basic_auth(&self.user, Some(&self.password))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("{method}: cannot reach bitcoind at {}", self.url))?;

        let json: Value = resp
            .json()
            .await
            .with_context(|| format!("{method}: bitcoind returned a non-JSON body"))?;

        if let Some(err) = json.get("error") {
            if !err.is_null() {
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                return Err(anyhow!("{method}: bitcoind said: {msg}"));
            }
        }

        json.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("{method}: bitcoind returned no result"))
    }

    /// Used at startup and in `get_info`. Also the check that produces a
    /// clear message when the wallet is empty or holds coins from a chain
    /// that no longer exists after a reset.
    pub async fn status(&self) -> Result<WalletStatus> {
        let info = self.call("getblockchaininfo", json!([])).await?;
        let balances = self.call("getbalances", json!([])).await?;

        let trusted_btc = balances
            .get("mine")
            .and_then(|m| m.get("trusted"))
            .and_then(|t| t.as_f64())
            .unwrap_or(0.0);

        Ok(WalletStatus {
            spendable_sat: (trusted_btc * 100_000_000.0).round() as u64,
            chain: info.get("chain").and_then(|c| c.as_str()).unwrap_or("").to_string(),
            block_height: info.get("blocks").and_then(|b| b.as_u64()).unwrap_or(0),
            block_hash: info
                .get("bestblockhash")
                .and_then(|h| h.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    /// Pay an address. The only method here that moves money.
    pub async fn send_to_address(&self, address: &str, amount_sat: u64) -> Result<String> {
        let btc = amount_sat as f64 / 100_000_000.0;
        let txid = self
            .call("sendtoaddress", json!([address, btc]))
            .await
            .with_context(|| format!("paying {amount_sat} sat to {address}"))?;
        txid.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("sendtoaddress did not return a txid"))
    }
}
