//! The control key.
//!
//! One key may change policy, pause and resume, without a restart. The first
//! thing an open faucet meets is somebody testing its edges, and the answer
//! to that should not be an SSH session into a container running a live
//! chain.
//!
//! Control travels on NCC (kinds 23198/23199), the same channel the rest of
//! the project uses for node control, rather than on the wallet channel —
//! so a wallet connection can never be mistaken for an administrative one.

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use nwc::nostr::nips::nip04;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::server::Faucet;

pub const CONTROL_REQUEST_KIND: u16 = 23198;
pub const CONTROL_RESPONSE_KIND: u16 = 23199;

#[derive(Debug, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ControlRequest {
    /// Stop paying. Requests are still answered, and refused with a reason.
    Pause,
    Resume,
    /// Change any limit. Omitted fields are left alone.
    SetPolicy(SetPolicy),
    /// What the policy currently is, and what has been paid out under it.
    Status,
}

#[derive(Debug, Deserialize)]
pub struct SetPolicy {
    pub per_key_sat: Option<u64>,
    pub window_secs: Option<u64>,
    pub total_cap_sat: Option<u64>,
    pub max_requests_per_window: Option<u32>,
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
    pub per_key_sat: u64,
    pub window_secs: u64,
    pub total_cap_sat: u64,
    pub max_requests_per_window: u32,
    pub paid_out_this_window_sat: u64,
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
            Some(hex) => PublicKey::parse(hex)
                .map(|k| k == sender)
                .unwrap_or(false),
            // No control key configured means nobody can control it, not
            // everybody. Failing closed is the only safe default for a
            // service that can spend a miner's wallet.
            None => false,
        }
    };

    let response = if !authorised {
        tracing::warn!("refusing control request from {sender} — not the control key");
        ControlResponse {
            ok: false,
            message: "not the control key".to_string(),
            policy: None,
        }
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

    let encrypted = nip04::encrypt(
        keys.secret_key(),
        &sender,
        serde_json::to_string(&response)?,
    )?;
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
                "paused".to_string()
            }
            ControlRequest::Resume => {
                cfg.policy.paused = false;
                "resumed".to_string()
            }
            ControlRequest::Status => "ok".to_string(),
            ControlRequest::SetPolicy(p) => {
                let mut changed = Vec::new();
                if let Some(v) = p.per_key_sat {
                    cfg.policy.per_key_sat = v;
                    changed.push(format!("per_key_sat={v}"));
                }
                if let Some(v) = p.window_secs {
                    cfg.policy.window_secs = v;
                    changed.push(format!("window_secs={v}"));
                }
                if let Some(v) = p.total_cap_sat {
                    cfg.policy.total_cap_sat = v;
                    changed.push(format!("total_cap_sat={v}"));
                }
                if let Some(v) = p.max_requests_per_window {
                    cfg.policy.max_requests_per_window = v;
                    changed.push(format!("max_requests_per_window={v}"));
                }
                if changed.is_empty() {
                    "nothing to change".to_string()
                } else {
                    tracing::info!("control: policy changed — {}", changed.join(", "));
                    format!("changed {}", changed.join(", "))
                }
            }
        }
    };

    ControlResponse { ok: true, message, policy: Some(snapshot(faucet).await) }
}

async fn snapshot(faucet: &Arc<Faucet>) -> PolicySnapshot {
    let cfg = faucet.cfg.lock().await.clone();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let paid_out = faucet
        .ledger
        .lock()
        .await
        .paid_out_in_window(cfg.policy.window_secs, now);
    let spendable = faucet.node.status().await.map(|s| s.spendable_sat).unwrap_or(0);

    PolicySnapshot {
        paused: cfg.policy.paused,
        per_key_sat: cfg.policy.per_key_sat,
        window_secs: cfg.policy.window_secs,
        total_cap_sat: cfg.policy.total_cap_sat,
        max_requests_per_window: cfg.policy.max_requests_per_window,
        paid_out_this_window_sat: paid_out,
        spendable_sat: spendable,
    }
}
