# nostr-faucet

Ask a signet miner for coins over Nostr.

A listed — today, any — key sends a BIP-321 `bitcoin:` URI as an NWC
`pay_bip321` request, and the miner pays it.

## What it is

An NWC wallet service implementing `pay_bip321`, on-chain branch, backed by
a `bitcoind` wallet rather than a Lightning node. The protocol is not new:
`pay_bip321` is already in NIP-47 in the [dukeh3/nostr][nostr] fork, and
`dln-node` already implements all three of its branches. This implements
one of them from a different backend.

`get_info` advertises `bip321_methods: [onchain]` and nothing else, so a
client can tell before it asks that this wallet will not pay a Lightning
invoice.

## Shape

**One server per miner.** Each instance holds RPC to exactly one `bitcoind`
and serves exactly one chain. There is no routing, so a request for one
chain cannot be paid from another's miner; the two fail independently; and
the RPC connection — which can spend a wallet holding tens of thousands of
coins — never leaves the machine.

**It dials out.** The faucet connects to a relay and never listens on a
port, so deploying it beside a miner adds nothing to that host's firewall.

## Policy

Open, at first: any key may ask for its allowance per window. Three
controls, each failing closed:

| Control | What it bounds |
|---|---|
| `per_key_sat` per `window_secs` | how much one key takes |
| `total_cap_sat` per window | how much the faucet pays out **at all** |
| `max_requests_per_window` | how often one key may ask, paid or refused |

**The cap is the one that matters.** Nostr pubkeys are free, so a per-key
quota is advisory against anyone willing to generate them. The cap bounds
the damage regardless, and is refused even to a key that is inside its own
quota.

Every refusal says why, and when to come back. A quota that silently drops
requests is a black hole — the asker cannot tell it from a faucet that is
down.

A whitelist is a later model. The policy decision lives in one function
(`policy::decide`), so adding one is a check at a known point rather than a
change threaded through the server.

## Control

One key, named in config, may pause, resume, change policy and read status
— without a restart, because the first thing an open faucet meets is
somebody testing its edges. Control travels on NCC (kinds 23198/23199), not
on the wallet channel, so a wallet connection can never be mistaken for an
administrative one.

If no control key is configured, **nobody** can control the faucet.

## What the node it points at must have

**`fallbackfee`**, in the node's `bitcoin.conf`. A chain with no
transaction history cannot estimate a fee, and without a fallback every
`sendtoaddress` fails outright. The faucet probes this at startup — by
funding a transaction it never signs or broadcasts — and says so before
it takes a single request:

```
this node cannot fund a payment, so every request will fail —
Fee estimation failed. Fallbackfee is disabled. …
```

**`rpcuser` and `rpcpassword`.** Cookie authentication is not supported.

## Running

```
nostr-faucet /etc/nostr-faucet/faucet.toml
```

See `faucet.example.toml`. Every limit is configuration rather than a
constant, deliberately: a one-week window cannot be tested against real
time, so the test suite sets it to seconds.

## Tests

`cargo test` covers the ledger and the policy decision. End-to-end
scenarios live in `nostr-faucet-e2e`, which runs both faucets against
locally spun regtest chains — the faucet cannot tell regtest from signet,
so that is a faithful test of everything it does except block timing.

[nostr]: https://github.com/dukeh3/nostr
