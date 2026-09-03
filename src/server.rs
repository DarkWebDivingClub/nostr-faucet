//! The NWC listener.
//!
//! Implements `pay_bip321`, on-chain branch only, and advertises exactly
//! that in `get_info` — a faucet with no Lightning wallet should say so in
//! its capabilities rather than failing a `bolt11` request at payment time.
//!
//! The faucet dials out to a relay. It never listens on a port, which is
//! why deploying it beside a miner adds nothing to that host's firewall.

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use nwc::nostr::nips::nip04;
use nwc::nostr::nips::nip47::{
    Bip321MethodInfo, ErrorCode, GetBalanceResponse, GetInfoResponse, Method, NIP47Error,
    PayBip321Response, Request, RequestParams, Response, ResponseResult,
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::bitcoind::Bitcoind;
use crate::config::Config;
use crate::grants::Grants;
use crate::policy::{self, Denial};
use crate::rate_limit::RateLimitRule;
use crate::state::Buckets;

pub struct Faucet {
    pub cfg: Mutex<Config>,
    pub node: Bitcoind,
    pub buckets: Mutex<Buckets>,
    /// Per-key allowances, as published by the owner. Per NNC.
    pub grants: Mutex<Grants>,
}

pub fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

pub async fn run(cfg: Config) -> Result<()> {
    let keys = Keys::parse(&cfg.nostr.secret_key).context("nostr.secret_key is not a valid key")?;
    let our_pubkey = keys.public_key();

    let node = Bitcoind::new(&cfg.bitcoind);

    // Fail at startup rather than at the first request. A faucet that cannot
    // reach its miner should say so now, while somebody is watching.
    let status = node
        .status()
        .await
        .context("cannot reach the miner's bitcoind — the faucet has nothing to pay with")?;
    tracing::info!(
        "{}: chain {} at height {}, {} sat spendable",
        node.chain_label,
        status.chain,
        status.block_height,
        status.spendable_sat
    );
    // Checked here rather than discovered on somebody's first request. A
    // faucet that cannot pay should say so while somebody is watching it
    // start, and say what to do about it.
    if let Err(why) = node.can_pay_fees().await {
        tracing::error!(
            "{}: this node cannot fund a payment, so every request will fail — {why}. \
             If that mentions fee estimation, set fallbackfee in its bitcoin.conf and \
             restart it; a chain with no transaction history cannot estimate one.",
            node.chain_label
        );
    }

    if status.spendable_sat == 0 {
        tracing::warn!(
            "{}: the miner wallet is empty. Requests will be refused with that reason \
             until it has mature coins.",
            node.chain_label
        );
    }

    let relay = cfg.nostr.relay.clone();
    let buckets = Buckets::new(&cfg.policy.total_cap, now_micros());
    let faucet = Arc::new(Faucet {
        cfg: Mutex::new(cfg),
        node,
        buckets: Mutex::new(buckets),
        grants: Mutex::new(Grants::new()),
    });

    let client = Client::builder().signer(keys.clone()).build();
    client.add_relay(&relay).await?;
    client.connect().await;
    tracing::info!("listening on {relay} as {our_pubkey}");

    // since=now: a faucet that restarts should not replay yesterday's asks.
    let filter = Filter::new()
        .kind(Kind::WalletConnectRequest)
        .pubkey(our_pubkey)
        .since(Timestamp::now());
    client.subscribe(filter).await?;

    // Control travels on its own kind, so a wallet connection can never be
    // mistaken for an administrative one.
    let control_filter = Filter::new()
        .kind(Kind::Custom(crate::control::CONTROL_REQUEST_KIND))
        .pubkey(our_pubkey)
        .since(Timestamp::now());
    client.subscribe(control_filter).await?;

    // Per-key allowances, published by the owner as kind-30078. No `since`:
    // a grant issued before this faucet started is still in force, and a
    // faucet that forgot everyone's allowance on restart would be wrong.
    let grants_filter = Filter::new().kind(Kind::Custom(30078)).pubkey(our_pubkey);
    client.subscribe(grants_filter).await?;

    let mut notifications = client.notifications();
    while let Some(notification) = notifications.next().await {
        if let ClientNotification::Event { event, .. } = notification {
            let event = event.as_ref();
            let is_wallet = event.kind == Kind::WalletConnectRequest;
            let is_control = event.kind == Kind::Custom(crate::control::CONTROL_REQUEST_KIND);
            let is_grant = event.kind == Kind::Custom(30078);
            if is_grant {
                faucet.grants.lock().await.apply(&our_pubkey.to_hex(), event);
                continue;
            }
            if !is_wallet && !is_control {
                continue;
            }
            let faucet = Arc::clone(&faucet);
            let client = client.clone();
            let keys = keys.clone();
            let event = event.clone();
            tokio::spawn(async move {
                let result = if is_control {
                    crate::control::handle(&faucet, &client, &keys, &event).await
                } else {
                    handle(&faucet, &client, &keys, &event).await
                };
                if let Err(e) = result {
                    tracing::warn!("failed to handle request: {e:#}");
                }
            });
        }
    }

    Ok(())
}

async fn handle(faucet: &Faucet, client: &Client, keys: &Keys, event: &Event) -> Result<()> {
    let asker = event.pubkey;
    let plaintext = nip04::decrypt(keys.secret_key(), &asker, &event.content)
        .context("could not decrypt request")?;
    let req: Request = serde_json::from_str(&plaintext).context("request is not valid NIP-47")?;

    let response = match &req.params {
        RequestParams::GetInfo => get_info(faucet).await,
        RequestParams::GetBalance => get_balance(faucet).await,
        RequestParams::PayBip321(p) => pay_bip321(faucet, &asker.to_hex(), &p.uri).await,
        other => {
            let method = format!("{other:?}");
            tracing::info!("refusing unsupported method from {asker}: {method}");
            Response {
                result_type: Method::GetInfo,
                error: Some(NIP47Error {
                    code: ErrorCode::NotImplemented,
                    message: "this faucet implements pay_bip321, get_info and get_balance only"
                        .to_string(),
                }),
                result: None,
            }
        }
    };

    let encrypted = nip04::encrypt(
        keys.secret_key(),
        &asker,
        serde_json::to_string(&response)?,
    )?;
    let reply = EventBuilder::new(Kind::WalletConnectResponse, encrypted)
        .tag(Tag::public_key(asker))
        .tag(Tag::event(event.id));
    client.send_event_builder(reply).await?;
    Ok(())
}

async fn get_info(faucet: &Faucet) -> Response {
    let status = faucet.node.status().await.ok();
    Response {
        result_type: Method::GetInfo,
        error: None,
        result: Some(ResponseResult::GetInfo(GetInfoResponse {
            alias: Some(format!("nostr-faucet ({})", faucet.node.chain_label)),
            color: None,
            pubkey: None,
            network: status.as_ref().map(|s| s.chain.clone()),
            block_height: status.as_ref().map(|s| s.block_height as u32),
            block_hash: status.as_ref().map(|s| s.block_hash.clone()),
            methods: vec![Method::GetInfo, Method::GetBalance, Method::PayBip321],
            notifications: vec![],
            // On-chain and nothing else. There is no Lightning wallet here,
            // and a client should be able to learn that before it asks.
            bip321_methods: Some(vec![Bip321MethodInfo {
                method: "onchain".into(),
                address_types: None,
            }]),
        })),
    }
}

async fn get_balance(faucet: &Faucet) -> Response {
    match faucet.node.status().await {
        Ok(s) => Response {
            result_type: Method::GetBalance,
            error: None,
            result: Some(ResponseResult::GetBalance(GetBalanceResponse {
                balance: s.spendable_sat.saturating_mul(1000),
                // No Lightning wallet here — the miner's on-chain balance
                // is the whole of it.
                lightning_balance: None,
                onchain_balance: Some(s.spendable_sat.saturating_mul(1000)),
            })),
        },
        Err(e) => err_response(Method::GetBalance, ErrorCode::Internal, &format!("{e:#}")),
    }
}

async fn pay_bip321(faucet: &Faucet, asker: &str, uri_str: &str) -> Response {
    let uri = match bip321::Uri::parse(uri_str) {
        Ok(u) => u,
        Err(e) => {
            return err_response(
                Method::PayBip321,
                ErrorCode::Other,
                &format!("not a valid BIP-321 URI: {e}"),
            )
        }
    };

    // On-chain only. Say which branch is missing rather than failing later.
    let Some(address) = uri.address_str() else {
        return err_response(
            Method::PayBip321,
            ErrorCode::NotImplemented,
            "this faucet pays on-chain only — the URI carries no address. \
             Its get_info advertises bip321_methods: [onchain].",
        );
    };
    let Some(amount) = uri.amount else {
        return err_response(
            Method::PayBip321,
            ErrorCode::Other,
            "an on-chain payment needs an amount in the URI",
        );
    };
    let amount_sat = amount.to_sat();

    let now = now_micros();
    let cfg = faucet.cfg.lock().await.clone();

    // The key's own grant, or the default that makes this faucet open.
    // `None` means no allowance at all — the whitelist case.
    let granted = faucet.grants.lock().await.get(asker).cloned();
    let profile = granted.as_ref().or(cfg.policy.default_profile.as_ref());

    // Rules to size a new key's buckets with. A key nobody has seen has
    // spent nothing, so its buckets start full.
    let quota_rule = profile
        .and_then(|p| p.quota.clone())
        .unwrap_or(RateLimitRule { rate_per_micro: 0, max_capacity: 0 });
    let rate_rule = profile
        .and_then(|p| p.access_rate.clone())
        .unwrap_or(RateLimitRule { rate_per_micro: 0, max_capacity: i64::MAX });

    let decision = {
        let mut b = faucet.buckets.lock().await;
        let q = b.quota_for(asker, &quota_rule, now).clone();
        let r = b.rate_for(asker, &rate_rule, now).clone();
        let t = b.total().clone();
        let d = policy::decide(&policy::Request {
            paused: cfg.policy.paused,
            amount_sat,
            now_micros: now,
            profile,
            quota_bucket: &q,
            rate_bucket: &r,
            total_bucket: &t,
            total_rule: &cfg.policy.total_cap,
        });
        // Every attempt costs a round trip, paid or not.
        b.charge_request(asker, &rate_rule, now);
        d
    };

    if let Err(denial) = decision {
        tracing::info!("refusing {asker}: {}", denial.message());
        let code = match denial {
            Denial::Paused => ErrorCode::Other,
            Denial::NotGranted => ErrorCode::Unauthorized,
            _ => ErrorCode::RateLimited,
        };
        return err_response(Method::PayBip321, code, &denial.message());
    }

    match faucet.node.send_to_address(address, amount_sat).await {
        Ok(txid) => {
            // Charged only now: a failed payment must not consume an
            // allowance somebody never received.
            faucet.buckets.lock().await.charge_payment(
                asker,
                amount_sat,
                &quota_rule,
                &cfg.policy.total_cap,
                now,
            );
            tracing::info!(
                "paid {amount_sat} sat to {address} for {asker} on {} — {txid}",
                faucet.node.chain_label
            );
            Response {
                result_type: Method::PayBip321,
                error: None,
                result: Some(ResponseResult::PayBip321(PayBip321Response {
                    preimage: None,
                    txid: Some(txid),
                    fees_paid: None,
                    payment_method: "onchain".to_string(),
                })),
            }
        }
        Err(e) => {
            tracing::warn!("payment failed for {asker}: {e:#}");
            err_response(
                Method::PayBip321,
                ErrorCode::PaymentFailed,
                &format!("the miner could not pay: {e:#}"),
            )
        }
    }
}

fn err_response(result_type: Method, code: ErrorCode, message: &str) -> Response {
    Response {
        result_type,
        error: Some(NIP47Error { code, message: message.to_string() }),
        result: None,
    }
}
