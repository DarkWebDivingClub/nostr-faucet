//! nostr-faucet — ask a signet miner for coins over Nostr.
//!
//! Two binaries share this crate: `nostr-faucet`, the server that sits
//! beside a miner, and `nostr-faucet-client`, which fills a treasury from
//! one. They ship together so the wire format cannot drift between them.
//!
//! The faucet is a **handler**. Everything protocol — grants, limits,
//! NIP-44, event kinds, the request pipeline — is `nostr-ln`, which
//! renders NIP-47 once for every service we run. This crate holds a
//! `bitcoind` client and the two methods it answers.

pub mod bitcoind;
pub mod client;
pub mod config;
pub mod server;
