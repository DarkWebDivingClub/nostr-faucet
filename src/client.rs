//! Filling a treasury from a faucet.
//!
//! This runs the inverse of a normal NWC flow, and that is why a wallet
//! extension cannot stand in for it. An extension has its own wallet and
//! would ask a faucet to pay an address it already controls. What we are
//! filling is a **treasury** — a `bitcoind` wallet on a chain — so the
//! address belongs to a node this client talks to over RPC, not to the
//! client itself.
//!
//! ```text
//!      bitcoind RPC                  NWC over Nostr
//! treasury ──getnewaddress──▶ client ──pay_onchain──▶ faucet ──▶ miner pays
//! ```
//!
//! Steps one and four both need RPC to the node being funded, which is
//! exactly what an extension does not have.

use anyhow::{Context, Result};
use nostr_ln::nwc::methods::PayOnchainResponse;
use nostr_ln::nwc::WalletMethod;
use nostr_sdk::prelude::*;
use serde_json::{json, Value};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

use crate::bitcoind::Bitcoind;
use crate::config::BitcoindConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct ClientConfig {
    pub faucet: FaucetConfig,
    pub bitcoind: BitcoindConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FaucetConfig {
    pub relay: String,
    /// The faucet's public key, hex or npub.
    pub pubkey: String,
    /// **Our** key, kept between runs on purpose.
    ///
    /// A fresh key every run would sail past a per-key quota, which is not
    /// so much clever as dishonest — the faucet's policy is "one coin a
    /// week per key", and a client that renames itself every week is
    /// helping itself to somebody else's coin.
    pub secret_key: String,
}

impl ClientConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read config at {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
    }
}

pub struct Outcome {
    pub txid: String,
    pub address: String,
    pub confirmations: Option<u64>,
}

/// Ask once, and wait for the money if asked to.
pub async fn fill(cfg: &ClientConfig, amount_btc: f64, wait: bool) -> Result<Outcome> {
    let keys = Keys::parse(&cfg.faucet.secret_key).context("faucet.secret_key is not a key")?;
    let faucet = PublicKey::parse(&cfg.faucet.pubkey).context("faucet.pubkey is not a key")?;
    let node = Bitcoind::new(&cfg.bitcoind);

    // 1. An address from the node being funded — not from us.
    //
    // bech32 explicitly: Knots defaults getnewaddress to legacy P2PKH,
    // which is not what anything else here uses.
    let address = node
        .new_address()
        .await
        .context("could not get an address from the node being funded")?;
    tracing::info!("asking for {amount_btc} to {address}");

    // 2. Ask.
    //
    // An address and an amount — no BIP-321 URI to build, and none for the
    // faucet to parse. `pay_onchain` is the method a wallet with no
    // Lightning can implement, and the one whose whole surface is two
    // fields.
    let amount_sats = (amount_btc * 100_000_000.0).round() as u64;
    let resp = round_trip(
        &cfg.faucet.relay,
        &keys,
        &faucet,
        WalletMethod::PayOnchain,
        json!({ "address": address, "amount_sats": amount_sats }),
    )
    .await?;

    if let Some(err) = resp.get("error").filter(|e| !e.is_null()) {
        // The faucet's reason, verbatim. Most of what goes wrong here is
        // somebody else's decision — not listed, over quota, faucet dry —
        // and turning that into our own words only loses information.
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("no message");
        anyhow::bail!("the faucet said no: {message}");
    }

    let result = resp.get("result").context("the faucet answered with neither result nor error")?;
    let paid: PayOnchainResponse = serde_json::from_value(result.clone())
        .context("the faucet's answer is not a pay_onchain response")?;
    let txid = paid.txid;

    if !wait {
        return Ok(Outcome { txid, address, confirmations: None });
    }

    // 4. A txid is a promise. We hold RPC to this chain, so watch it.
    tracing::info!("waiting for {txid} to confirm — a txid is not money");
    let confirmations = node
        .wait_for_confirmation(&txid, Duration::from_secs(600))
        .await?;
    Ok(Outcome { txid, address, confirmations: Some(confirmations) })
}

/// Send one NWC request and read its answer.
///
/// Hand-rolled because `nostr-ln` has no NWC client yet — that belongs
/// with mission 11.2, which is about this client. What matters here is
/// that it is **NIP-44**: this used NIP-04 until 2026-09-06, which is
/// [dln-node#3](https://github.com/DarkWebDivingClub/dln-node/issues/3)'s
/// bug in another repository, and `nostr-ln` refuses a NIP-04 request
/// rather than guessing.
async fn round_trip(
    relay: &str,
    us: &Keys,
    faucet: &PublicKey,
    method: WalletMethod,
    params: Value,
) -> Result<Value> {
    const REQUEST_KIND: u16 = 23194;
    const RESPONSE_KIND: u16 = 23195;

    let client = Client::builder().signer(us.clone()).build();
    client.add_relay(relay).await?;
    client.connect().await;

    // Before the send: a response can arrive before a later subscription.
    client
        .subscribe(
            Filter::new()
                .kind(Kind::Custom(RESPONSE_KIND))
                .pubkey(us.public_key())
                .since(Timestamp::now()),
        )
        .await?;

    let payload = json!({ "method": method.as_str(), "params": params }).to_string();
    let ciphertext = us.nip44_encrypt(faucet, &payload).await?;
    let event = EventBuilder::new(Kind::Custom(REQUEST_KIND), ciphertext)
        .tag(Tag::public_key(*faucet))
        .sign(us)
        .await?;
    let request_id = event.id;
    client.send_event(&event).await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        anyhow::ensure!(!left.is_zero(), "the faucet did not answer within 30s");
        let found = client
            .fetch_events(
                Filter::new()
                    .kind(Kind::Custom(RESPONSE_KIND))
                    .pubkey(us.public_key())
                    .event(request_id),
            )
            .timeout(Duration::from_secs(2))
            .await?;
        if let Some(e) = found.first() {
            let plain = us.nip44_decrypt(&e.pubkey, &e.content).await?;
            client.shutdown().await;
            return Ok(serde_json::from_str(&plain)?);
        }
    }
}
