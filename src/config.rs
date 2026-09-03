//! Configuration.
//!
//! Every limit is configuration rather than a constant, and that is a
//! deliberate testability requirement: a "one coin a week" policy cannot be
//! tested against real time. The suite sets `window_secs` to seconds and
//! `total_cap_sat` to a few coins, and exhausts both inside one run.

use anyhow::{Context, Result};
use crate::grants::UsageProfile;
use crate::rate_limit::RateLimitRule;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub nostr: NostrConfig,
    pub bitcoind: BitcoindConfig,
    pub policy: PolicyConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NostrConfig {
    /// Relay to listen on. The faucet dials out; it never listens on a port.
    pub relay: String,
    /// The faucet's own secret key, hex or nsec.
    pub secret_key: String,
    /// The key permitted to change policy, pause and revoke. Everything it
    /// can do is refused to every other key, including a key that is
    /// otherwise inside its quota.
    pub control_pubkey: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BitcoindConfig {
    /// Kept on localhost by design. This connection can spend the miner's
    /// wallet, and one server per miner is what makes never exposing it
    /// possible.
    #[serde(default = "default_rpc_host")]
    pub rpc_host: String,
    pub rpc_port: u16,
    pub rpc_user: String,
    pub rpc_password: String,
    /// The miner's wallet — what the faucet pays out of.
    pub wallet: String,
    /// Reported in `get_info` and used to refuse a request aimed at the
    /// wrong chain. Both signets report "signet", so this is a label.
    pub chain_label: String,
}

fn default_rpc_host() -> String {
    "127.0.0.1".to_string()
}

/// The faucet's default policy.
///
/// `default_profile` is applied to any key with no grant of its own, which
/// is what makes the policy open. Remove it and the faucet becomes a
/// whitelist: a key with no grant has no allowance.
///
/// Per-key configuration otherwise arrives as kind-30078 grants, per NCC —
/// this service does not invent a way to set it.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConfig {
    /// Applied to keys without a grant. Omit to require a grant.
    pub default_profile: Option<UsageProfile>,
    /// What the faucet pays out in total, across every key.
    ///
    /// The control that matters: a per-key quota is advisory against anyone
    /// willing to generate keys, which costs nothing on Nostr.
    pub total_cap: RateLimitRule,
    #[serde(default)]
    pub paused: bool,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read config at {}", path.display()))?;
        let cfg: Config = toml::from_str(&text)
            .with_context(|| format!("cannot parse config at {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.policy.total_cap.max_capacity > 0,
            "policy.total_cap.max_capacity must be > 0, or no request could ever succeed"
        );
        if self.policy.default_profile.is_none() {
            tracing::warn!(
                "no policy.default_profile — this faucet will refuse every key that does \
                 not hold a grant. That is the whitelist model; set one to be open."
            );
        }
        if self.bitcoind.rpc_host != "127.0.0.1" && self.bitcoind.rpc_host != "localhost" {
            tracing::warn!(
                "bitcoind.rpc_host is {} — this connection can spend the miner's wallet \
                 and is meant to stay on localhost",
                self.bitcoind.rpc_host
            );
        }
        Ok(())
    }
}
