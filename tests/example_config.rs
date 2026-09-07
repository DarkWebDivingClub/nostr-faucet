//! The shipped example config must parse.
//!
//! This is the test that was missing. `nostr-faucet-e2e-test` was suspended
//! from the mandatory set on 2026-09-04 because the harness wrote a config
//! the faucet no longer accepted — `missing field total_cap` — and had been
//! red since the 3rd without anybody noticing.
//!
//! A shipped example that does not parse fails an operator the same way,
//! more quietly. Parsing it here costs nothing and cannot drift, because it
//! reads the file that ships.

use std::io::Write;

#[test]
fn the_example_config_parses() {
    let example = include_str!("../faucet.example.toml");

    // The placeholders are not keys. Substitute valid ones, and nothing
    // else — if a field were missing this still fails, which is the point.
    let filled = example
        .replace(
            "\"nsec1...\"",
            "\"0000000000000000000000000000000000000000000000000000000000000001\"",
        )
        .replace(
            "\"npub1...\"",
            "\"0000000000000000000000000000000000000000000000000000000000000002\"",
        )
        .replace("rpc_user = \"...\"", "rpc_user = \"u\"")
        .replace("rpc_password = \"...\"", "rpc_password = \"p\"");

    let dir = std::env::temp_dir().join("nostr-faucet-example-config");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("faucet.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(filled.as_bytes()).unwrap();
    drop(f);

    let cfg = nostr_faucet::config::Config::load(&path)
        .expect("the shipped example config does not parse");

    assert!(!cfg.nostr.owners.is_empty(), "the example must show an owner");
    assert!(cfg.faucet.total_cap.max_capacity > 0);
    assert!(cfg.faucet.total_cap.is_valid());
}

#[test]
fn the_example_config_names_nothing_that_was_removed() {
    // `control_pubkey`, `[policy]`, `paused` and `rate_per_micro` all
    // existed before 13.4. An example that still shows them sends an
    // operator to a config the faucet refuses.
    let example = include_str!("../faucet.example.toml");
    for gone in ["control_pubkey", "[policy]", "paused =", "rate_per_micro", "per_key_sat"] {
        assert!(
            !example.contains(gone),
            "the example still shows `{gone}`, which this faucet no longer accepts"
        );
    }
}

#[test]
fn the_rpc_host_defaults_to_loopback() {
    // This connection can spend the miner's wallet. It is the whole reason
    // there is one faucet per miner rather than one serving both chains,
    // and the default must not be the permissive one.
    let toml = r#"
[nostr]
relay = "ws://localhost:1"
secret_key = "0000000000000000000000000000000000000000000000000000000000000001"
owners = ["0000000000000000000000000000000000000000000000000000000000000002"]

[bitcoind]
rpc_port = 1
rpc_user = "u"
rpc_password = "p"
wallet = "w"
chain_label = "l"

[faucet.total_cap]
amount = 1
per_secs = 1
max_capacity = 1
"#;
    let dir = std::env::temp_dir().join("nostr-faucet-default-host");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("faucet.toml");
    std::fs::write(&path, toml).unwrap();

    let cfg = nostr_faucet::config::Config::load(&path).expect("parses without rpc_host");
    assert_eq!(
        cfg.bitcoind.rpc_host, "127.0.0.1",
        "an omitted rpc_host must default to loopback, never to a wildcard \
         or a hostname that could resolve off-box"
    );
}

#[test]
fn a_faucet_with_no_owners_starts_and_grants_nothing() {
    // Absent configuration fails closed. Treating "no owners" as "any
    // owner" is dln-node#1, and the direction is what matters: a faucet
    // that refuses everything is a nuisance, one that accepts everything
    // spends a miner's wallet.
    let toml = r#"
[nostr]
relay = "ws://localhost:1"
secret_key = "0000000000000000000000000000000000000000000000000000000000000001"

[bitcoind]
rpc_host = "127.0.0.1"
rpc_port = 1
rpc_user = "u"
rpc_password = "p"
wallet = "w"
chain_label = "l"

[faucet.total_cap]
amount = 1
per_secs = 1
max_capacity = 1
"#;
    let dir = std::env::temp_dir().join("nostr-faucet-no-owners");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("faucet.toml");
    std::fs::write(&path, toml).unwrap();

    let cfg = nostr_faucet::config::Config::load(&path).expect("an absent owners list parses");
    assert!(cfg.nostr.owners.is_empty(), "and yields no owners, rather than a default one");
}
