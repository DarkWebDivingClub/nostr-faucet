//! The quota ledger.
//!
//! Records what was asked and what was paid, per key and in total, inside a
//! rolling window. Held in memory: the window is short relative to how long
//! a faucet stays up, and a restart forgiving everyone's quota is a better
//! failure than a faucet that will not start because its ledger is corrupt.
//!
//! Requests are recorded whether or not they were paid, because a refused
//! flood still costs the faucet and the relay.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
struct Entry {
    at: u64,
    paid_sat: u64,
}

#[derive(Debug, Default)]
pub struct Ledger {
    per_key: HashMap<String, Vec<Entry>>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an attempt. `paid_sat` is zero for a refusal, which still
    /// counts against the request rate.
    pub fn record(&mut self, key: &str, paid_sat: u64, now: u64) {
        self.per_key.entry(key.to_string()).or_default().push(Entry { at: now, paid_sat });
    }

    /// Drop everything older than the window, so the map does not grow
    /// without bound on a long-running faucet.
    pub fn prune(&mut self, window_secs: u64, now: u64) {
        let cutoff = now.saturating_sub(window_secs);
        self.per_key.retain(|_, entries| {
            entries.retain(|e| e.at > cutoff);
            !entries.is_empty()
        });
    }

    pub fn requests_in_window(&self, key: &str, window_secs: u64, now: u64) -> u32 {
        let cutoff = now.saturating_sub(window_secs);
        self.per_key
            .get(key)
            .map(|es| es.iter().filter(|e| e.at > cutoff).count() as u32)
            .unwrap_or(0)
    }

    pub fn paid_to_key_in_window(&self, key: &str, window_secs: u64, now: u64) -> u64 {
        let cutoff = now.saturating_sub(window_secs);
        self.per_key
            .get(key)
            .map(|es| es.iter().filter(|e| e.at > cutoff).map(|e| e.paid_sat).sum())
            .unwrap_or(0)
    }

    pub fn paid_out_in_window(&self, window_secs: u64, now: u64) -> u64 {
        let cutoff = now.saturating_sub(window_secs);
        self.per_key
            .values()
            .flat_map(|es| es.iter())
            .filter(|e| e.at > cutoff)
            .map(|e| e.paid_sat)
            .sum()
    }

    /// When this key's oldest entry falls out of the window. Used to tell an
    /// asker when to come back rather than leaving them to guess.
    pub fn key_window_resets_in(&self, key: &str, window_secs: u64, now: u64) -> u64 {
        let cutoff = now.saturating_sub(window_secs);
        self.per_key
            .get(key)
            .and_then(|es| es.iter().filter(|e| e.at > cutoff).map(|e| e.at).min())
            .map(|oldest| (oldest + window_secs).saturating_sub(now))
            .unwrap_or(0)
    }

    pub fn global_window_resets_in(&self, window_secs: u64, now: u64) -> u64 {
        let cutoff = now.saturating_sub(window_secs);
        self.per_key
            .values()
            .flat_map(|es| es.iter())
            .filter(|e| e.at > cutoff && e.paid_sat > 0)
            .map(|e| e.at)
            .min()
            .map(|oldest| (oldest + window_secs).saturating_sub(now))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_count_against_rate_but_not_quota() {
        let mut l = Ledger::new();
        l.record("alice", 0, 100);
        l.record("alice", 0, 101);
        assert_eq!(l.requests_in_window("alice", 60, 110), 2);
        assert_eq!(l.paid_to_key_in_window("alice", 60, 110), 0);
    }

    #[test]
    fn the_window_rolls() {
        let mut l = Ledger::new();
        l.record("alice", 500, 100);
        assert_eq!(l.paid_to_key_in_window("alice", 60, 130), 500);
        // 100 is now outside a 60s window ending at 200
        assert_eq!(l.paid_to_key_in_window("alice", 60, 200), 0);
    }

    #[test]
    fn the_cap_counts_every_key() {
        let mut l = Ledger::new();
        l.record("alice", 500, 100);
        l.record("bob", 700, 101);
        assert_eq!(l.paid_out_in_window(60, 110), 1200);
    }

    #[test]
    fn pruning_keeps_the_map_bounded() {
        let mut l = Ledger::new();
        l.record("alice", 500, 100);
        l.prune(60, 500);
        assert_eq!(l.paid_out_in_window(60, 500), 0);
        assert!(l.per_key.is_empty());
    }
}
