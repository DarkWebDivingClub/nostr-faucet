//! Live buckets, one pair per key plus one for the faucet.
//!
//! A key's buckets are created full the first time it asks, from whichever
//! rule applies to it — its grant, or the default. That is the right
//! default: a key nobody has seen has spent nothing.
//!
//! Held in memory. The window is short relative to how long a faucet stays
//! up, and a restart forgiving everyone's quota is a better failure than a
//! faucet that will not start because its ledger is corrupt.

use std::collections::HashMap;

use crate::rate_limit::{Bucket, RateLimitRule};

pub struct Buckets {
    quota: HashMap<String, Bucket>,
    rate: HashMap<String, Bucket>,
    total: Bucket,
}

impl Buckets {
    pub fn new(total_rule: &RateLimitRule, now_micros: u64) -> Self {
        Self {
            quota: HashMap::new(),
            rate: HashMap::new(),
            total: Bucket::full(total_rule, now_micros),
        }
    }

    pub fn quota_for(&mut self, key: &str, rule: &RateLimitRule, now: u64) -> &Bucket {
        self.quota.entry(key.to_string()).or_insert_with(|| Bucket::full(rule, now))
    }

    pub fn rate_for(&mut self, key: &str, rule: &RateLimitRule, now: u64) -> &Bucket {
        self.rate.entry(key.to_string()).or_insert_with(|| Bucket::full(rule, now))
    }

    pub fn total(&self) -> &Bucket {
        &self.total
    }

    /// A request was made. Counted whether or not it was paid, because a
    /// refused flood still costs the faucet and the relay a round trip.
    pub fn charge_request(&mut self, key: &str, rule: &RateLimitRule, now: u64) {
        self.rate
            .entry(key.to_string())
            .or_insert_with(|| Bucket::full(rule, now))
            .withdraw(1, now, rule);
    }

    /// Coins actually moved. Charged only after the payment succeeded, so a
    /// failed payment does not consume somebody's allowance.
    pub fn charge_payment(
        &mut self,
        key: &str,
        amount: u64,
        quota_rule: &RateLimitRule,
        total_rule: &RateLimitRule,
        now: u64,
    ) {
        self.quota
            .entry(key.to_string())
            .or_insert_with(|| Bucket::full(quota_rule, now))
            .withdraw(amount, now, quota_rule);
        self.total.withdraw(amount, now, total_rule);
    }

    pub fn keys_seen(&self) -> usize {
        self.quota.len().max(self.rate.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COIN: u64 = 100_000_000;

    fn rule() -> RateLimitRule {
        RateLimitRule { rate_per_micro: 0, max_capacity: COIN as i64 }
    }

    #[test]
    fn a_new_key_starts_with_a_full_allowance() {
        let r = rule();
        let mut b = Buckets::new(&r, 0);
        assert!(b.quota_for("newcomer", &r, 0).can_withdraw(COIN, 0, &r));
    }

    #[test]
    fn a_failed_payment_does_not_consume_an_allowance() {
        // charge_payment is only called after bitcoind says yes.
        let r = rule();
        let mut b = Buckets::new(&r, 0);
        b.charge_request("alice", &r, 0);
        assert!(b.quota_for("alice", &r, 0).can_withdraw(COIN, 0, &r));
    }

    #[test]
    fn a_payment_charges_both_the_key_and_the_faucet() {
        let r = rule();
        let mut b = Buckets::new(&r, 0);
        b.charge_payment("alice", COIN, &r, &r, 0);
        assert!(!b.quota_for("alice", &r, 0).can_withdraw(1, 0, &r));
        assert!(!b.total().can_withdraw(1, 0, &r));
    }
}
