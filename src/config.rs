//! Configuration.
//!
//! Every limit is configuration rather than a constant, and that is a
//! deliberate testability requirement: a "one coin a week" policy cannot be
//! tested against real time. The suite sets `window_secs` to seconds and
//! `total_cap_sat` to a few coins, and exhausts both inside one run.

use anyhow::{Context, Result};
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

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConfig {
    /// What one key may take per window.
    pub per_key_sat: u64,
    /// The window, in seconds. A week in production; seconds in tests.
    pub window_secs: u64,
    /// What the faucet will pay out in total per window, across every key.
    ///
    /// **This is the control that matters.** A per-key quota is advisory
    /// against anyone willing to generate keys, which costs nothing on
    /// Nostr. The cap bounds the damage regardless, and is refused even to
    /// a key inside its own quota.
    pub total_cap_sat: u64,
    /// Requests one key may make per window, paid or refused.
    ///
    /// Payout limits do not bound a script that asks a thousand times and
    /// is refused a thousand times — that still costs the faucet and the
    /// relay a thousand round trips.
    pub max_requests_per_window: u32,
    /// Start paused. The control key can unpause without a restart.
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
        anyhow::ensure!(self.policy.window_secs > 0, "policy.window_secs must be > 0");
        anyhow::ensure!(self.policy.per_key_sat > 0, "policy.per_key_sat must be > 0");
        anyhow::ensure!(
            self.policy.total_cap_sat >= self.policy.per_key_sat,
            "policy.total_cap_sat ({}) is below per_key_sat ({}) — no request could ever \
             succeed, since the first payout would exceed the cap",
            self.policy.total_cap_sat,
            self.policy.per_key_sat
        );
        anyhow::ensure!(
            self.policy.max_requests_per_window > 0,
            "policy.max_requests_per_window must be > 0"
        );
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
