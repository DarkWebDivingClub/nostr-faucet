//! Capability grants, as per NCC.
//!
//! Per-key configuration is a kind-30078 event published by the owner, not
//! something this service invents. The event's `d` tag addresses it:
//!
//! ```text
//! d = <faucet_pubkey>:<asker_pubkey>
//! ```
//!
//! and its content is a `UsageProfile` — the same structure `dln-node`
//! applies, so a grant means the same thing wherever it lands.
//!
//! This replaces a `set_policy` control method I had written before
//! checking whether NCC already answered the question. It did.
//!
//! ## Open today, whitelist later
//!
//! The faucet holds a **default profile** from its config, applied to any
//! key with no grant of its own. That is what makes the policy open: a
//! stranger is treated as if granted the default.
//!
//! Switching to a whitelist later is then removing the default rather than
//! writing new code — a key with no grant simply has no allowance.

use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::rate_limit::RateLimitRule;

/// What a grant carries. The same shape as `dln-node`'s, minus the parts a
/// faucet has no use for: it implements one paid method, so a per-method
/// map would have one entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageProfile {
    /// How much this key may take, as a token bucket over sats.
    pub quota: Option<RateLimitRule>,
    /// How often it may ask at all, paid or refused. A refused flood still
    /// costs the faucet and the relay a round trip each.
    pub access_rate: Option<RateLimitRule>,
}

#[derive(Debug, Default)]
pub struct Grants {
    profiles: HashMap<String, UsageProfile>,
    /// Applied event ids with their timestamps, so a replayed or
    /// out-of-order grant cannot undo a newer one.
    applied: HashMap<String, u64>,
}

impl Grants {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, pubkey: &str) -> Option<&UsageProfile> {
        self.profiles.get(pubkey)
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Apply a kind-30078 event, if it is addressed to us and is not stale.
    ///
    /// Returns the key it applies to, for logging.
    pub fn apply(&mut self, our_pubkey: &str, event: &Event) -> Option<String> {
        let target = grant_target(our_pubkey, event)?;

        let created = event.created_at.as_secs();
        let id = event.id.to_string();
        if let Some(&seen) = self.applied.get(&target) {
            if created < seen {
                tracing::debug!("ignoring a grant for {target} older than the one applied");
                return None;
            }
        }

        match serde_json::from_str::<UsageProfile>(&event.content) {
            Ok(profile) => {
                self.applied.insert(target.clone(), created);
                self.profiles.insert(target.clone(), profile);
                tracing::info!("applied grant {id} for {target}");
                Some(target)
            }
            Err(e) => {
                tracing::warn!("grant {id} for {target} is not a usage profile: {e}");
                None
            }
        }
    }
}

/// `d = <faucet_pubkey>:<asker_pubkey>`, and only if the first half is us.
///
/// A grant naming another node is not ours to apply, however well-formed.
fn grant_target(our_pubkey: &str, event: &Event) -> Option<String> {
    let d = event.tags.iter().find_map(|t| {
        let p = t.as_slice();
        (p.first().map(|v| v.as_str()) == Some("d")).then(|| p.get(1).cloned()).flatten()
    })?;
    let (node, user) = d.split_once(':')?;
    if node != our_pubkey || user.is_empty() {
        return None;
    }
    Some(user.to_string())
}
