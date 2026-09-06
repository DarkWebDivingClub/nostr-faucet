//! The faucet, as a handler and nothing else.
//!
//! It receives a method name, typed parameters, and who is asking. It does
//! not touch Nostr: no relay, no NIP-44, no grants, no buckets, no event
//! kinds. All of that is `nostr-ln`, which renders NIP-47 once for every
//! service we run — the whole point of
//! [user story 11](https://github.com/DarkWebDivingClub).
//!
//! What used to be here — `grants.rs`, `policy.rs`, `rate_limit.rs`,
//! `state.rs`, `control.rs`, and most of this file — was a private
//! rendering of the same protocol, and it had diverged from the
//! specification in five ways.
//!
//! ## The one limit that is still ours
//!
//! `total_cap` is enforced here rather than by the access layer, and the
//! distinction is real: a grant answers *may this controller do this*,
//! which `nostr-ln` enforces per controller. `total_cap` answers *can this
//! faucet afford it at all*, which is a property of the faucet. It is
//! checked in [`prepare`](FaucetService::prepare) so it is checked
//! **before** anything is paid.

use std::sync::Arc;

use anyhow::Result;
use nostr_ln::nnc::{ErrorCode, NncError};
use nostr_ln::nwc::methods::*;
use nostr_ln::service::handler::Fut;
use nostr_ln::service::{Caller, Prepared, Service, WalletService};
use nostr_ln::{Bucket, RateLimitRule};
use nostr_sdk::prelude::{Keys, PublicKey};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::bitcoind::Bitcoind;
use crate::config::Config;

pub struct FaucetService {
    node: Bitcoind,
    chain_label: String,
    total_cap: RateLimitRule,
    /// What the faucet has paid out, in sats, against `total_cap`.
    ///
    /// In memory. The window is short relative to how long a faucet stays
    /// up, and a restart forgiving the cap is a better failure than a
    /// faucet that will not start because its ledger is corrupt.
    spent: Mutex<Bucket>,
}

impl FaucetService {
    pub fn new(cfg: &Config) -> Self {
        let total_cap = cfg.faucet.total_cap.clone();
        Self {
            node: Bitcoind::new(&cfg.bitcoind),
            chain_label: cfg.bitcoind.chain_label.clone(),
            total_cap: total_cap.clone(),
            spent: Mutex::new(Bucket::full(&total_cap, now_secs())),
        }
    }
}

#[nostr_ln::service]
impl WalletService for FaucetService {
    fn get_balance<'a>(&'a self, _r: GetBalanceRequest, _c: Caller<'a>)
        -> Fut<'a, Result<GetBalanceResponse, NncError>>
    {
        Box::pin(async move {
            let status = self
                .node
                .status()
                .await
                .map_err(|e| NncError::new(ErrorCode::Internal, format!("{e:#}")))?;
            // Published core's get_balance is one field, in msats. A faucet
            // holds on-chain funds and no channels, so this is all of it.
            Ok(GetBalanceResponse { balance: status.spendable_sat.saturating_mul(1_000) })
        })
    }

    fn get_info<'a>(&'a self, _r: GetInfoRequest, _c: Caller<'a>)
        -> Fut<'a, Result<GetInfoResponse, NncError>>
    {
        Box::pin(async move {
            Ok(GetInfoResponse {
                alias: Some(format!("nostr-faucet ({})", self.chain_label)),
                color: None,
                pubkey: None,
                network: Some(self.chain_label.clone()),
                block_height: None,
                block_hash: None,
                methods: self.methods().iter().map(|m| m.to_string()).collect(),
                // `pay_onchain`'s specification has no number yet, so it
                // appears in `methods` and nowhere else.
                extensions: None,
            })
        })
    }

    fn pay_onchain<'a>(&'a self, r: PayOnchainRequest, c: Caller<'a>)
        -> Fut<'a, Result<PayOnchainResponse, NncError>>
    {
        Box::pin(async move {
            let txid = self
                .node
                .send_to_address(&r.address, r.amount_sats)
                .await
                .map_err(|e| {
                    NncError::new(ErrorCode::PaymentFailed, format!("the miner could not pay: {e:#}"))
                })?;

            // Charged only after the miner paid: a failed payment must not
            // consume an allowance nobody received. The per-key quota is
            // charged by the pipeline on the same rule.
            self.spent.lock().await.withdraw(r.amount_sats, now_secs(), &self.total_cap);

            tracing::info!(
                "paid {} sat to {} for {} on {} — {txid}",
                r.amount_sats,
                r.address,
                c.controller,
                self.chain_label
            );
            Ok(PayOnchainResponse { txid, fee_sats: None })
        })
    }

    /// Quote the cost, and refuse what this faucet cannot afford.
    ///
    /// Two checks that must happen **before** anything is paid, and that
    /// the access layer cannot make for us:
    ///
    /// - the faucet's own `total_cap`, which is not a grant
    /// - whether `bitcoind` can pay the fees at all
    ///
    /// Returning the cost is also what makes the per-key quota absolute:
    /// the pipeline checks it against this figure rather than against
    /// something guessed from the request.
    fn prepare<'a>(&'a self, method: &'a str, params: &'a Value, _c: Caller<'a>)
        -> Fut<'a, Result<Prepared, NncError>>
    {
        Box::pin(async move {
            if method != "pay_onchain" {
                return Ok(Prepared::free());
            }
            let request: PayOnchainRequest = serde_json::from_value(params.clone())
                .map_err(|e| NncError::new(ErrorCode::Other, format!("bad params: {e}")))?;

            let now = now_secs();
            let remaining = {
                let spent = self.spent.lock().await;
                if !spent.can_withdraw(request.amount_sats, now, &self.total_cap) {
                    Some(spent.balance_at(now, &self.total_cap))
                } else {
                    None
                }
            };
            if let Some(left) = remaining {
                return Err(NncError::new(
                    ErrorCode::QuotaExceeded,
                    format!(
                        "this faucet has {left} sat left before its total cap; \
                         {} was asked for",
                        request.amount_sats
                    ),
                ));
            }

            if let Err(why) = self.node.can_pay_fees().await {
                return Err(NncError::new(ErrorCode::InsufficientBalance, why));
            }

            Ok(Prepared::new(request.amount_sats, ()))
        })
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Connect and serve until stopped.
pub async fn run(cfg: Config) -> Result<()> {
    let keys = Keys::parse(&cfg.nostr.secret_key)?;
    let owners: Vec<PublicKey> = cfg
        .nostr
        .owners
        .iter()
        .map(|o| PublicKey::parse(o))
        .collect::<Result<_, _>>()?;

    tracing::info!(
        "faucet {} on {} — relay {}, {} owner(s)",
        keys.public_key(),
        cfg.bitcoind.chain_label,
        cfg.nostr.relay,
        owners.len()
    );

    let handler = Arc::new(FaucetService::new(&cfg));
    Service::new(keys, vec![cfg.nostr.relay.clone()], owners)
        .wallet(handler)
        .run()
        .await?;
    Ok(())
}
