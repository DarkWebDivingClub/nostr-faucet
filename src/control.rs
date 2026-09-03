//! The control key.
//!
//! One key may stop and start payouts without a restart. The first thing an
//! open faucet meets is somebody testing its edges, and the answer to that
//! should not be an SSH session into a container running a live chain.
//!
//! It does **not** set policy. Per-key allowances are kind-30078 grants,
//! per NNC — see `grants.rs`. An earlier version of this had a `set_policy`
//! method, written before checking whether NNC already answered that
//! question. It did.
//!
//! Control travels on NNC (kinds 23198/23199), the same channel the rest of
//! the project uses for node control, rather than on the wallet channel —
//! so a wallet connection can never be mistaken for an administrative one.

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use nwc::nostr::nips::nip04;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::grants::UsageProfile;
use crate::rate_limit::RateLimitRule;
use crate::server::{now_micros, Faucet};

pub const CONTROL_REQUEST_KIND: u16 = 23198;
pub const CONTROL_RESPONSE_KIND: u16 = 23199;

/// What the control key may do — only what a grant cannot express.
#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum ControlRequest {
    /// Stop paying. Requests are still answered, and refused with a reason.
    Pause,
    Resume,
    Status,
}

#[derive(Debug, Serialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<PolicySnapshot>,
}

#[derive(Debug, Serialize)]
pub struct PolicySnapshot {
    pub paused: bool,
    /// The allowance a key gets with no grant of its own. `None` means this
    /// faucet is a whitelist.
    pub default_profile: Option<UsageProfile>,
    pub total_cap: RateLimitRule,
    pub total_remaining_sat: i64,
    /// How many keys hold a grant of their own.
    pub grants_in_force: usize,
    pub keys_seen: usize,
    pub spendable_sat: u64,
}

/// Handle one control request.
///
/// The authorisation check is the first thing here and has no other caller:
/// a key that is not *the* control key is refused, however generous the
/// payout policy happens to be.
pub async fn handle(
    faucet: &Arc<Faucet>,
    client: &Client,
    keys: &Keys,
    event: &Event,
) -> Result<()> {
    let sender = event.pubkey;

    let authorised = {
        let cfg = faucet.cfg.lock().await;
        match cfg.nostr.control_pubkey.as_deref() {
            Some(hex) => PublicKey::parse(hex).map(|k| k == sender).unwrap_or(false),
            // No control key configured means nobody can control it, not
            // everybody. Failing closed is the only safe default for a
            // service that can spend a miner's wallet.
            None => false,
        }
    };

    let response = if !authorised {
        tracing::warn!("refusing control request from {sender} — not the control key");
        ControlResponse { ok: false, message: "not the control key".into(), policy: None }
    } else {
        let plaintext = nip04::decrypt(keys.secret_key(), &sender, &event.content)
            .context("could not decrypt control request")?;
        match serde_json::from_str::<ControlRequest>(&plaintext) {
            Ok(req) => apply(faucet, req).await,
            Err(e) => ControlResponse {
                ok: false,
                message: format!("could not parse control request: {e}"),
                policy: None,
            },
        }
    };

    let encrypted =
        nip04::encrypt(keys.secret_key(), &sender, serde_json::to_string(&response)?)?;
    let reply = EventBuilder::new(Kind::Custom(CONTROL_RESPONSE_KIND), encrypted)
        .tag(Tag::public_key(sender))
        .tag(Tag::event(event.id));
    client.send_event_builder(reply).await?;
    Ok(())
}

async fn apply(faucet: &Arc<Faucet>, req: ControlRequest) -> ControlResponse {
    let message = {
        let mut cfg = faucet.cfg.lock().await;
        match req {
            ControlRequest::Pause => {
                cfg.policy.paused = true;
                tracing::warn!("control: paused");
                "paused".to_string()
            }
            ControlRequest::Resume => {
                cfg.policy.paused = false;
                tracing::info!("control: resumed");
                "resumed".to_string()
            }
            ControlRequest::Status => "ok".to_string(),
        }
    };
    ControlResponse { ok: true, message, policy: Some(snapshot(faucet).await) }
}

async fn snapshot(faucet: &Arc<Faucet>) -> PolicySnapshot {
    let cfg = faucet.cfg.lock().await.clone();
    let now = now_micros();
    let (total_remaining, keys_seen) = {
        let b = faucet.buckets.lock().await;
        (b.total().balance_at(now, &cfg.policy.total_cap), b.keys_seen())
    };
    let grants_in_force = faucet.grants.lock().await.len();
    let spendable = faucet.node.status().await.map(|s| s.spendable_sat).unwrap_or(0);

    PolicySnapshot {
        paused: cfg.policy.paused,
        default_profile: cfg.policy.default_profile.clone(),
        total_cap: cfg.policy.total_cap.clone(),
        total_remaining_sat: total_remaining,
        grants_in_force,
        keys_seen,
        spendable_sat: spendable,
    }
}
