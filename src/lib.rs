//! nostr-faucet — ask a signet miner for coins over Nostr.
//!
//! Two binaries share this crate: `nostr-faucet`, the server that sits
//! beside a miner, and `nostr-faucet-client`, which fills a treasury from
//! one. They ship together so the wire format cannot drift between them.

pub mod bitcoind;
pub mod client;
pub mod config;
pub mod control;
pub mod policy;
pub mod server;
pub mod state;
