//! The policy decision, in one place.
//!
//! Everything that decides whether a request is paid goes through
//! [`decide`]. That is deliberate: the first model is open — any key may ask
//! for its allowance per window — and a whitelist is a later model. Keeping
//! the decision in one function means adding that whitelist is one more
//! check at a known point rather than a change threaded through the server.
//!
//! The asking key is carried through even though today it is used only for
//! quota accounting, for the same reason.

use crate::config::PolicyConfig;
use crate::state::Ledger;

/// Why a request was refused. Every variant produces a message that says
/// what happened and, where there is one, what to do about it.
///
/// A whitelist that silently drops requests is a black hole; so is a quota.
/// The asker cannot tell "refused" from "faucet is down" unless we say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denial {
    Paused,
    OverKeyQuota { taken: u64, allowance: u64, retry_in_secs: u64 },
    OverTotalCap { paid_out: u64, cap: u64, retry_in_secs: u64 },
    TooManyRequests { made: u32, allowed: u32, retry_in_secs: u64 },
    AmountTooLarge { asked: u64, max: u64 },
    /// Reserved for the whitelist model. Unreachable while the policy is
    /// open, and present so the shape of that model is already decided.
    #[allow(dead_code)]
    NotListed,
}

impl Denial {
    pub fn message(&self) -> String {
        match self {
            Denial::Paused => "the faucet is paused".to_string(),
            Denial::OverKeyQuota { taken, allowance, retry_in_secs } => format!(
                "over your quota: {taken} of {allowance} sat taken this window, \
                 try again in {retry_in_secs}s"
            ),
            Denial::OverTotalCap { paid_out, cap, retry_in_secs } => format!(
                "the faucet has paid out {paid_out} of {cap} sat this window and is \
                 capped, try again in {retry_in_secs}s — this is not about your key"
            ),
            Denial::TooManyRequests { made, allowed, retry_in_secs } => format!(
                "too many requests: {made} of {allowed} this window, \
                 try again in {retry_in_secs}s"
            ),
            Denial::AmountTooLarge { asked, max } => {
                format!("asked for {asked} sat, the most this faucet pays at once is {max}")
            }
            Denial::NotListed => "this key is not listed".to_string(),
        }
    }
}

/// The single decision point.
///
/// `now` is passed rather than read so the tests can drive the window
/// without waiting for it.
pub fn decide(
    key: &str,
    amount_sat: u64,
    cfg: &PolicyConfig,
    ledger: &Ledger,
    now: u64,
) -> Result<(), Denial> {
    if cfg.paused {
        return Err(Denial::Paused);
    }

    // A whitelist check would go here, and nowhere else.

    if amount_sat > cfg.per_key_sat {
        return Err(Denial::AmountTooLarge { asked: amount_sat, max: cfg.per_key_sat });
    }

    // Rate first: a refused request should cost as little as possible, and
    // counting it before the more expensive checks is the cheapest place to
    // stop a flood.
    let requests = ledger.requests_in_window(key, cfg.window_secs, now);
    if requests >= cfg.max_requests_per_window {
        return Err(Denial::TooManyRequests {
            made: requests,
            allowed: cfg.max_requests_per_window,
            retry_in_secs: ledger.key_window_resets_in(key, cfg.window_secs, now),
        });
    }

    let taken = ledger.paid_to_key_in_window(key, cfg.window_secs, now);
    if taken + amount_sat > cfg.per_key_sat {
        return Err(Denial::OverKeyQuota {
            taken,
            allowance: cfg.per_key_sat,
            retry_in_secs: ledger.key_window_resets_in(key, cfg.window_secs, now),
        });
    }

    // Last, and refused even to a key inside its own quota. This is the
    // control that actually bounds a script, since new keys are free.
    let paid_out = ledger.paid_out_in_window(cfg.window_secs, now);
    if paid_out + amount_sat > cfg.total_cap_sat {
        return Err(Denial::OverTotalCap {
            paid_out,
            cap: cfg.total_cap_sat,
            retry_in_secs: ledger.global_window_resets_in(cfg.window_secs, now),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PolicyConfig;

    fn cfg() -> PolicyConfig {
        PolicyConfig {
            per_key_sat: 100_000_000, // 1 coin
            window_secs: 60,
            total_cap_sat: 250_000_000, // 2.5 coins across everyone
            max_requests_per_window: 3,
            paused: false,
        }
    }

    #[test]
    fn a_key_that_has_never_asked_is_paid() {
        // The policy is open: no listing step stands between a new key and
        // its first coin.
        let l = Ledger::new();
        assert!(decide("newcomer", 100_000_000, &cfg(), &l, 1000).is_ok());
    }

    #[test]
    fn a_second_coin_in_the_window_is_refused() {
        let mut l = Ledger::new();
        l.record("alice", 100_000_000, 1000);
        let d = decide("alice", 100_000_000, &cfg(), &l, 1010).unwrap_err();
        assert!(matches!(d, Denial::OverKeyQuota { .. }));
        assert!(d.message().contains("over your quota"));
    }

    #[test]
    fn the_quota_frees_up_once_the_window_rolls() {
        let mut l = Ledger::new();
        l.record("alice", 100_000_000, 1000);
        assert!(decide("alice", 100_000_000, &cfg(), &l, 1100).is_ok());
    }

    #[test]
    fn the_cap_refuses_a_key_inside_its_own_quota() {
        // The point of the global cap. Two other keys have taken 2 coins,
        // so a third key asking for its first coin exceeds the 2.5 cap even
        // though it has taken nothing itself.
        let mut l = Ledger::new();
        l.record("bob", 100_000_000, 1000);
        l.record("carol", 100_000_000, 1001);
        let c = PolicyConfig { total_cap_sat: 250_000_000, ..cfg() };
        let d = decide("alice", 100_000_000, &c, &l, 1010).unwrap_err();
        assert!(matches!(d, Denial::OverTotalCap { .. }));
        // The refusal has to say this is not the asker's fault, or they will
        // keep retrying a request that cannot succeed.
        assert!(d.message().contains("not about your key"));
    }

    #[test]
    fn a_refused_flood_is_stopped_by_the_rate_limit() {
        // Refusals cost nothing to the asker but cost us a round trip each,
        // so they count even though no coins moved.
        let mut l = Ledger::new();
        for t in 0..3 {
            l.record("flooder", 0, 1000 + t);
        }
        let d = decide("flooder", 1, &cfg(), &l, 1010).unwrap_err();
        assert!(matches!(d, Denial::TooManyRequests { .. }));
    }

    #[test]
    fn paused_refuses_everyone() {
        let c = PolicyConfig { paused: true, ..cfg() };
        let d = decide("alice", 1, &c, &Ledger::new(), 1000).unwrap_err();
        assert_eq!(d, Denial::Paused);
    }

    #[test]
    fn asking_for_more_than_the_allowance_is_refused_up_front() {
        let d = decide("alice", 200_000_000, &cfg(), &Ledger::new(), 1000).unwrap_err();
        assert!(matches!(d, Denial::AmountTooLarge { .. }));
    }

    #[test]
    fn every_refusal_says_something_actionable() {
        // A whitelist or quota that silently drops requests is a black hole.
        let mut l = Ledger::new();
        l.record("alice", 100_000_000, 1000);
        let d = decide("alice", 100_000_000, &cfg(), &l, 1010).unwrap_err();
        let m = d.message();
        assert!(m.contains("try again in"), "refusal should say when: {m}");
    }
}
