//! The policy decision, in one place.
//!
//! Everything that decides whether a request is paid goes through
//! [`decide`]. What it consults is a key's [`UsageProfile`] — its grant if
//! it has one, otherwise the faucet's default.
//!
//! That default is what makes the policy open: a key nobody has ever heard
//! of is treated as if granted it. Switching to a whitelist later is
//! removing the default, not writing new code — a key with no grant would
//! then have no allowance and be refused for that stated reason.

use crate::grants::UsageProfile;
use crate::rate_limit::Bucket;

/// Why a request was refused. Every variant produces a message that says
/// what happened and, where there is one, what to do about it.
///
/// A quota that silently drops requests is a black hole. The asker cannot
/// tell "refused" from "faucet is down" unless we say which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denial {
    Paused,
    NotGranted,
    OverQuota { retry_in_secs: u64 },
    TooManyRequests { retry_in_secs: u64 },
    OverTotalCap { retry_in_secs: u64 },
}

fn when(secs: u64) -> String {
    if secs == u64::MAX {
        "this allowance does not refill — ask the operator".to_string()
    } else {
        format!("try again in {secs}s")
    }
}

impl Denial {
    pub fn message(&self) -> String {
        match self {
            Denial::Paused => "the faucet is paused".to_string(),
            Denial::NotGranted => {
                "this key has no allowance on this faucet — ask the operator for a grant"
                    .to_string()
            }
            Denial::OverQuota { retry_in_secs } => {
                format!("over your quota — {}", when(*retry_in_secs))
            }
            Denial::TooManyRequests { retry_in_secs } => {
                format!("too many requests — {}", when(*retry_in_secs))
            }
            Denial::OverTotalCap { retry_in_secs } => format!(
                "the faucet is at its overall cap for now, which is not about your key — {}",
                when(*retry_in_secs)
            ),
        }
    }
}

/// What the decision needs to look at. Passed in rather than reached for, so
/// the caller holds the locks and the tests hold nothing.
pub struct Request<'a> {
    pub paused: bool,
    pub amount_sat: u64,
    pub now_micros: u64,
    /// The key's grant, or the faucet's default for a key without one.
    /// `None` means no allowance at all — the whitelist case.
    pub profile: Option<&'a UsageProfile>,
    pub quota_bucket: &'a Bucket,
    pub rate_bucket: &'a Bucket,
    /// The faucet-wide cap. The control that actually bounds a script,
    /// since new keys cost nothing on Nostr.
    pub total_bucket: &'a Bucket,
    pub total_rule: &'a crate::rate_limit::RateLimitRule,
}

/// The single decision point.
pub fn decide(req: &Request) -> Result<(), Denial> {
    if req.paused {
        return Err(Denial::Paused);
    }

    // No grant and no default: the whitelist case. Unreachable while a
    // default profile is configured, which is what "open" means.
    let Some(profile) = req.profile else {
        return Err(Denial::NotGranted);
    };

    // Rate first: a refused request should cost as little as possible, so
    // stop a flood before doing the more expensive work.
    if let Some(rule) = &profile.access_rate {
        if !req.rate_bucket.can_withdraw(1, req.now_micros, rule) {
            let bal = req.rate_bucket.balance_at(req.now_micros, rule);
            return Err(Denial::TooManyRequests { retry_in_secs: rule.seconds_until(bal, 1) });
        }
    }

    if let Some(rule) = &profile.quota {
        if !req.quota_bucket.can_withdraw(req.amount_sat, req.now_micros, rule) {
            let bal = req.quota_bucket.balance_at(req.now_micros, rule);
            let want = i64::try_from(req.amount_sat).unwrap_or(i64::MAX);
            return Err(Denial::OverQuota { retry_in_secs: rule.seconds_until(bal, want) });
        }
    }

    // Last, and refused even to a key well inside its own quota.
    if !req.total_bucket.can_withdraw(req.amount_sat, req.now_micros, req.total_rule) {
        let bal = req.total_bucket.balance_at(req.now_micros, req.total_rule);
        let want = i64::try_from(req.amount_sat).unwrap_or(i64::MAX);
        return Err(Denial::OverTotalCap {
            retry_in_secs: req.total_rule.seconds_until(bal, want),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limit::RateLimitRule;

    const SEC: u64 = 1_000_000;
    const COIN: u64 = 100_000_000;

    fn profile() -> UsageProfile {
        UsageProfile {
            quota: Some(RateLimitRule {
                rate_per_micro: COIN / (604_800 * SEC),
                max_capacity: COIN as i64,
            }),
            access_rate: Some(RateLimitRule { rate_per_micro: 0, max_capacity: 3 }),
        }
    }

    fn total() -> RateLimitRule {
        RateLimitRule { rate_per_micro: 0, max_capacity: (5 * COIN) as i64 }
    }

    fn req<'a>(
        p: Option<&'a UsageProfile>,
        q: &'a Bucket,
        r: &'a Bucket,
        t: &'a Bucket,
        tr: &'a RateLimitRule,
        amount: u64,
        now: u64,
    ) -> Request<'a> {
        Request {
            paused: false,
            amount_sat: amount,
            now_micros: now,
            profile: p,
            quota_bucket: q,
            rate_bucket: r,
            total_bucket: t,
            total_rule: tr,
        }
    }

    #[test]
    fn a_key_with_the_default_profile_is_paid() {
        // What "open" means: a key nobody has heard of gets the default.
        let p = profile();
        let (tr, q, r) = (total(), Bucket::full(p.quota.as_ref().unwrap(), 0),
                          Bucket::full(p.access_rate.as_ref().unwrap(), 0));
        let t = Bucket::full(&tr, 0);
        assert!(decide(&req(Some(&p), &q, &r, &t, &tr, COIN, 0)).is_ok());
    }

    #[test]
    fn a_key_with_no_profile_at_all_is_refused() {
        // The whitelist case, reachable only once the default is removed.
        let tr = total();
        let p = profile();
        let (q, r) = (Bucket::full(p.quota.as_ref().unwrap(), 0),
                      Bucket::full(p.access_rate.as_ref().unwrap(), 0));
        let t = Bucket::full(&tr, 0);
        let d = decide(&req(None, &q, &r, &t, &tr, COIN, 0)).unwrap_err();
        assert_eq!(d, Denial::NotGranted);
        assert!(d.message().contains("no allowance"));
    }

    #[test]
    fn a_second_coin_inside_the_window_is_refused() {
        let p = profile();
        let qr = p.quota.as_ref().unwrap();
        let mut q = Bucket::full(qr, 0);
        q.withdraw(COIN, 0, qr);
        let (tr, r) = (total(), Bucket::full(p.access_rate.as_ref().unwrap(), 0));
        let t = Bucket::full(&tr, 0);
        let d = decide(&req(Some(&p), &q, &r, &t, &tr, COIN, SEC)).unwrap_err();
        assert!(matches!(d, Denial::OverQuota { .. }));
        // Not "try again in N" — see the test below. A weekly refill rounds
        // to zero in NCC's integer rate, so this allowance does not refill
        // on its own, and the message says so instead of naming a time that
        // would never arrive.
        assert!(d.message().contains("does not refill"), "{}", d.message());
    }

    #[test]
    fn ncc_cannot_express_a_weekly_refill_and_we_do_not_pretend_otherwise() {
        // RateLimitRule.rate_per_micro is an integer number of units per
        // microsecond. One coin a week is 100_000_000 sat over
        // 604_800_000_000 us — 0.000165 sat/us, which truncates to zero.
        // The smallest expressible non-zero rate is 1 sat/us, or a million
        // sat a second.
        //
        // So a grant meaning "one coin a week" is really "one coin, until
        // the operator republishes the grant". That is a property of the
        // shared type, not of this faucet, and the refusal message must not
        // promise a refill that will never come.
        let weekly = RateLimitRule {
            rate_per_micro: COIN / (604_800 * SEC),
            max_capacity: COIN as i64,
        };
        assert_eq!(weekly.rate_per_micro, 0, "a weekly rate truncates to zero");
        assert_eq!(weekly.seconds_until(0, COIN as i64), u64::MAX);
    }

    #[test]
    fn the_cap_refuses_a_key_inside_its_own_quota() {
        let p = profile();
        let tr = total();
        let mut t = Bucket::full(&tr, 0);
        t.withdraw(5 * COIN, 0, &tr); // everyone else drained it
        let (q, r) = (Bucket::full(p.quota.as_ref().unwrap(), 0),
                      Bucket::full(p.access_rate.as_ref().unwrap(), 0));
        let d = decide(&req(Some(&p), &q, &r, &t, &tr, COIN, SEC)).unwrap_err();
        assert!(matches!(d, Denial::OverTotalCap { .. }));
        assert!(d.message().contains("not about your key"));
    }

    #[test]
    fn a_refused_flood_is_stopped_by_the_rate_limit() {
        let p = profile();
        let rr = p.access_rate.as_ref().unwrap();
        let mut r = Bucket::full(rr, 0);
        r.withdraw(3, 0, rr);
        let (tr, q) = (total(), Bucket::full(p.quota.as_ref().unwrap(), 0));
        let t = Bucket::full(&tr, 0);
        let d = decide(&req(Some(&p), &q, &r, &t, &tr, 1, SEC)).unwrap_err();
        assert!(matches!(d, Denial::TooManyRequests { .. }));
    }

    #[test]
    fn a_limit_that_never_refills_says_so_rather_than_lying() {
        let rr = RateLimitRule { rate_per_micro: 0, max_capacity: 1 };
        assert!(rr.seconds_until(0, 1) == u64::MAX);
        let d = Denial::OverQuota { retry_in_secs: u64::MAX };
        assert!(d.message().contains("does not refill"));
    }
}
