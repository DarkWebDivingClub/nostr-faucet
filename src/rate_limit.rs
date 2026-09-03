//! Token bucket, matching NCC's `RateLimitRule`.
//!
//! Deliberately the same shape and arithmetic `dln-node` uses, rather than a
//! second rate limiter with its own semantics. Two limiters in one project
//! is how behaviour drifts: the same grant should mean the same thing
//! wherever it is applied.
//!
//! `rate_per_micro` tokens are added per microsecond elapsed, capped at
//! `max_capacity`. A bucket starts full.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitRule {
    #[serde(default)]
    pub rate_per_micro: u64,
    #[serde(default = "default_max_capacity")]
    pub max_capacity: i64,
}

fn default_max_capacity() -> i64 {
    i64::MAX
}

impl RateLimitRule {
    /// How long until `amount` could be withdrawn, in seconds. Used to tell
    /// an asker when to come back rather than leaving them to guess.
    pub fn seconds_until(&self, balance: i64, amount: i64) -> u64 {
        if balance >= amount {
            return 0;
        }
        if self.rate_per_micro == 0 {
            // Never refills. Saying "try again in 0s" would be a lie.
            return u64::MAX;
        }
        let needed = (amount - balance) as u64;
        needed.div_ceil(self.rate_per_micro) / 1_000_000
    }
}

#[derive(Debug, Clone)]
pub struct Bucket {
    balance: i64,
    last_refill_micros: u64,
}

impl Bucket {
    pub fn full(rule: &RateLimitRule, now_micros: u64) -> Self {
        Self { balance: rule.max_capacity, last_refill_micros: now_micros }
    }

    pub fn balance_at(&self, now_micros: u64, rule: &RateLimitRule) -> i64 {
        let elapsed = now_micros.saturating_sub(self.last_refill_micros);
        let added = rule.rate_per_micro.saturating_mul(elapsed);
        let added = i64::try_from(added).unwrap_or(i64::MAX);
        self.balance.saturating_add(added).min(rule.max_capacity)
    }

    /// Can `amount` be taken now? Checked separately from taking it, because
    /// a payment can fail after the check and must not consume the balance.
    pub fn can_withdraw(&self, amount: u64, now_micros: u64, rule: &RateLimitRule) -> bool {
        match i64::try_from(amount) {
            Ok(a) => self.balance_at(now_micros, rule) >= a,
            Err(_) => false,
        }
    }

    /// Take it. Only called once a payment has actually succeeded.
    pub fn withdraw(&mut self, amount: u64, now_micros: u64, rule: &RateLimitRule) {
        let projected = self.balance_at(now_micros, rule);
        let a = i64::try_from(amount).unwrap_or(i64::MAX);
        self.balance = projected.saturating_sub(a);
        self.last_refill_micros = now_micros;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: u64 = 1_000_000;

    fn one_coin_a_week() -> RateLimitRule {
        // 100_000_000 sat over 604_800 s. Integer division means the rate is
        // slightly under, which errs toward refusing — the right direction.
        RateLimitRule {
            rate_per_micro: 100_000_000 / (604_800 * SEC),
            max_capacity: 100_000_000,
        }
    }

    #[test]
    fn a_full_bucket_covers_its_capacity() {
        let r = one_coin_a_week();
        let b = Bucket::full(&r, 0);
        assert!(b.can_withdraw(100_000_000, 0, &r));
    }

    #[test]
    fn a_spent_bucket_refuses_a_second_take() {
        let r = one_coin_a_week();
        let mut b = Bucket::full(&r, 0);
        b.withdraw(100_000_000, 0, &r);
        assert!(!b.can_withdraw(100_000_000, SEC, &r));
    }

    #[test]
    fn a_bucket_never_exceeds_its_capacity() {
        let r = RateLimitRule { rate_per_micro: 1_000, max_capacity: 500 };
        let b = Bucket { balance: 0, last_refill_micros: 0 };
        assert_eq!(b.balance_at(1_000 * SEC, &r), 500);
    }

    #[test]
    fn a_bucket_that_never_refills_says_so() {
        let r = RateLimitRule { rate_per_micro: 0, max_capacity: 100 };
        assert_eq!(r.seconds_until(0, 100), u64::MAX);
    }
}
