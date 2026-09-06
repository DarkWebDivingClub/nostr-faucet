//! Configuration.
//!
//! Almost everything that used to live here is now a **grant**: who may
//! ask, how often, and how much. The faucet does not invent a way to set
//! per-key policy, because NNC already answered that question — the owner
//! publishes kind `30198` and the faucet obeys.
//!
//! What is left is what a grant cannot express.

use std::path::Path;

use anyhow::{Context, Result};
use nostr_ln::RateLimitRule;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub nostr: NostrConfig,
    pub bitcoind: BitcoindConfig,
    pub faucet: FaucetConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NostrConfig {
    /// Relay to listen on. The faucet dials out; it never listens on a port.
    pub relay: String,
    /// The faucet's own secret key, hex or nsec.
    pub secret_key: String,
    /// The keys whose grants this faucet accepts.
    ///
    /// **Empty accepts nothing**, and therefore answers nothing.
    /// `nostr-ln` enforces that: absent configuration fails closed rather
    /// than treating "no owners" as "any owner", which is
    /// [dln-node#1](https://github.com/DarkWebDivingClub/dln-node/issues/1).
    ///
    /// Renamed from `control_pubkey` — it is plural, and it grants rather
    /// than controls.
    #[serde(default)]
    pub owners: Vec<String>,
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

/// What a grant cannot say.
#[derive(Debug, Clone, Deserialize)]
pub struct FaucetConfig {
    /// What this faucet pays out in total, across every key.
    ///
    /// **Deliberately not a grant.** A grant answers "may this controller
    /// do this", and the access layer enforces it per controller. This
    /// answers "can this faucet afford it at all", which is a property of
    /// the faucet rather than of anyone asking — closer to insufficient
    /// balance than to authorization.
    ///
    /// It is also the control that matters. A per-key quota is advisory
    /// against anyone willing to generate keys, which costs nothing on
    /// Nostr; this is the number that bounds the loss.
    pub total_cap: RateLimitRule,
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
            self.faucet.total_cap.max_capacity > 0,
            "faucet.total_cap.max_capacity must be > 0, or no request could ever succeed"
        );
        anyhow::ensure!(
            self.faucet.total_cap.is_valid(),
            "faucet.total_cap is not a valid rate limit rule"
        );
        if self.nostr.owners.is_empty() {
            tracing::warn!(
                "no nostr.owners — this faucet accepts no grants and will therefore \
                 refuse every request. That is fail-closed, not a bug, but it is \
                 probably not what was meant."
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
