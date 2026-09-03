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
//! treasury ──getnewaddress──▶ client ──pay_bip321──▶ faucet ──▶ miner pays
//! ```
//!
//! Steps one and four both need RPC to the node being funded, which is
//! exactly what an extension does not have.

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use nwc::nostr::nips::nip04;
use nwc::nostr::nips::nip47::{Method, PayBip321Request, Request, RequestParams, Response};
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

    // 2. A BIP-321 URI is the payment request.
    let uri = format!("bitcoin:{address}?amount={amount_btc}");

    // 3. Ask.
    let resp = round_trip(
        &cfg.faucet.relay,
        &keys,
        &faucet,
        Request {
            method: Method::PayBip321,
            params: RequestParams::PayBip321(PayBip321Request { uri }),
        },
    )
    .await?;

    if let Some(err) = resp.error {
        // The faucet's reason, verbatim. Most of what goes wrong here is
        // somebody else's decision — not listed, over quota, faucet dry —
        // and turning that into our own words only loses information.
        anyhow::bail!("the faucet said no: {}", err.message);
    }

    let value = serde_json::to_value(&resp)?;
    let txid = value
        .pointer("/result/txid")
        .and_then(|t| t.as_str())
        .context("the faucet answered without a txid")?
        .to_string();

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

async fn round_trip(
    relay: &str,
    us: &Keys,
    faucet: &PublicKey,
    req: Request,
) -> Result<Response> {
    let client = Client::builder().signer(us.clone()).build();
    client.add_relay(relay).await?;
    client.connect().await;

    client
        .subscribe(
            Filter::new()
                .kind(Kind::WalletConnectResponse)
                .pubkey(us.public_key())
                .since(Timestamp::now()),
        )
        .await?;

    let encrypted = nip04::encrypt(us.secret_key(), faucet, serde_json::to_string(&req)?)?;
    client
        .send_event_builder(
            EventBuilder::new(Kind::WalletConnectRequest, encrypted).tag(Tag::public_key(*faucet)),
        )
        .await?;

    let mut notifications = client.notifications();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        // Silence is not refusal. A faucet that is down and one that said no
        // are different problems with different fixes, and conflating them
        // sends people looking in the wrong place.
        anyhow::ensure!(
            !remaining.is_zero(),
            "no answer from the faucet in 45s — it may be down, on another relay, \
             or not the key in faucet.pubkey. This is not a refusal."
        );
        let next = tokio::time::timeout(remaining, notifications.next()).await;
        let Ok(Some(ClientNotification::Event { event, .. })) = next else { continue };
        if event.kind != Kind::WalletConnectResponse || event.pubkey != *faucet {
            continue;
        }
        let plaintext = nip04::decrypt(us.secret_key(), faucet, &event.content)?;
        let _ = client.disconnect().await;
        return Ok(serde_json::from_str(&plaintext)?);
    }
}
