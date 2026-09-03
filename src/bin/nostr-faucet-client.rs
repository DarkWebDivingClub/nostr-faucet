//! nostr-faucet-client — fill a treasury from a faucet.
//!
//! ```text
//! nostr-faucet-client [config.toml] [amount]
//! ```

use anyhow::{Context, Result};
use nostr_faucet::client::{self, ClientConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("faucet-client.toml"));
    let amount: f64 = args.next().unwrap_or_else(|| "1".into()).parse()
        .context("amount must be a number, in whole coins")?;

    let cfg = ClientConfig::load(&path)?;
    let wait = std::env::var("NO_WAIT").is_err();

    match client::fill(&cfg, amount, wait).await {
        Ok(o) => {
            println!("paid {amount} to {}", o.address);
            println!("txid {}", o.txid);
            match o.confirmations {
                Some(c) => println!("confirmed ({c})"),
                None => println!("not waiting for confirmation — a txid is not money"),
            }
            Ok(())
        }
        Err(e) => {
            // The faucet's own words, not ours.
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    }
}
