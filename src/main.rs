//! nostr-faucet — ask a signet miner for coins over Nostr.
//!
//! One server per miner. It holds RPC to exactly one `bitcoind` and serves
//! exactly one chain, so there is no path by which a request for one chain
//! could be paid from another's miner, and the RPC never leaves the machine.

mod bitcoind;
mod config;
mod control;
mod policy;
mod server;
mod state;

use anyhow::{Context, Result};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/nostr-faucet/faucet.toml"));

    let cfg = config::Config::load(&path)
        .with_context(|| format!("loading {}", path.display()))?;

    server::run(cfg).await
}
