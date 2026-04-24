<div align="center">

# ⬡ Vela Network

**A custom Layer-1 blockchain built from scratch in Rust.**

[![Testnet Live](https://img.shields.io/badge/testnet-live-22c55e?style=flat-square)](https://vela-network-production.up.railway.app)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-ef4444?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![License MIT](https://img.shields.io/badge/license-MIT-38bdf8?style=flat-square)](LICENSE)

HotStuff BFT consensus · libp2p networking · Ed25519 signatures · Blake3 hashing

No forks. No SDKs. Every line written from scratch.

[**Explorer**](https://vela-network-production.up.railway.app) · [**Wallet**](https://ordex78.github.io/vela-network/wallet.html) · [**Faucet**](https://ordex78.github.io/vela-network/faucet.html) · [**Whitepaper**](https://ordex78.github.io/vela-network/whitepaper.html)

</div>

---

## Overview

Vela is an independent Layer-1 blockchain with its own consensus engine, networking stack, transaction model, and state machine. It runs a live testnet with a block explorer, web wallet, and faucet.

| | |
|---|---|
| **Token** | VELA (1 VELA = 1,000,000,000 nVELA) |
| **Consensus** | HotStuff BFT (3-phase commit, linear complexity) |
| **Networking** | libp2p — Gossipsub, mDNS, Noise encryption |
| **Signatures** | Ed25519 (RFC 8032) |
| **Hashing** | Blake3 |
| **Storage** | sled embedded database |
| **Addresses** | `vela:` + 32-byte hex public key |

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      Vela Node                          │
│                                                         │
│  ┌───────────┐  ┌───────────┐  ┌──────────────────────┐│
│  │ Consensus │  │  Network  │  │     RPC / Explorer   ││
│  │ HotStuff  │  │  libp2p   │  │     Axum HTTP API    ││
│  │   BFT     │  │ Gossipsub │  │                      ││
│  └─────┬─────┘  └─────┬─────┘  └──────────┬───────────┘│
│        │              │                    │            │
│  ┌─────┴──────────────┴────────────────────┴───────────┐│
│  │                   Mempool                           ││
│  └─────────────────────┬───────────────────────────────┘│
│                        │                                │
│  ┌─────────────────────┴───────────────────────────────┐│
│  │            World State + Block Storage              ││
│  │          (sled DB · balance · nonce · txs)          ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
```

---

## Project Structure

```
src/
├── main.rs              # Node entry point, validator setup, block production
├── types.rs             # Block, Transaction, Hash, Address, Vote, QuorumCert
├── consensus/
│   └── hotstuff.rs      # HotStuff BFT — 3-phase voting, QC formation, leader rotation
├── network/
│   └── mod.rs           # libp2p node — Gossipsub, mDNS, peer management, chain sync
├── rpc/
│   ├── mod.rs           # Axum HTTP API — status, blocks, balance, send_tx, faucet
│   └── explorer.html    # Built-in block explorer served at /
├── storage/
│   ├── db.rs            # sled-backed persistent block storage
│   └── state.rs         # World state — balances, nonces, faucet cooldowns
├── crypto/              # Keypair utilities
├── mempool/             # Transaction pool
docs/
├── index.html           # Landing page (GitHub Pages)
├── wallet.html          # Web wallet
├── faucet.html          # Testnet faucet
└── whitepaper.html      # Whitepaper
```

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)

### Run a single node

```bash
git clone https://github.com/OrDex78/vela-network
cd vela-network
cargo run --bin vela-node
```

The node starts on port **8001** (P2P) and **9001** (RPC/Explorer).

Open the explorer: [http://localhost:9001](http://localhost:9001)

### Run 3 validators locally

Open three terminals:

```bash
# Terminal 1 — Validator 0 (leader for round 0)
cargo run --bin vela-node -- --port 8001 --validator-index 0

# Terminal 2 — Validator 1
cargo run --bin vela-node -- --port 8002 --validator-index 1 --bootstrap /ip4/127.0.0.1/tcp/8001

# Terminal 3 — Validator 2
cargo run --bin vela-node -- --port 8003 --validator-index 2 --bootstrap /ip4/127.0.0.1/tcp/8001
```

Nodes discover each other via mDNS on the same machine, or connect via the `--bootstrap` flag. Each node produces blocks when it's the round leader and syncs missing blocks on startup.

---

## HotStuff BFT Consensus

HotStuff achieves **linear message complexity** — O(n) messages per round vs O(n²) in PBFT.

**Three phases per block:**

1. **Prepare** — Leader proposes a block, replicas vote
2. **Pre-Commit** — Leader aggregates votes into a Quorum Certificate (QC)
3. **Commit** — Block is finalized with 2f+1 matching votes

With `n = 3f + 1` validators, the protocol tolerates up to `f` Byzantine faults. Leader rotation is round-robin across the validator set.

---

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/status` | Node status, block height, mempool size |
| GET | `/block/{height}` | Block details by height |
| GET | `/balance/{address}` | Balance and nonce for an address |
| GET | `/transactions/{address}` | Transaction history for an address |
| GET | `/validators` | Validator set and current leader |
| POST | `/send_tx` | Submit a signed transaction |
| POST | `/faucet/{address}` | Request testnet tokens |

---

## Roadmap

- [x] Core types — Block, Transaction, Hash, Vote, QuorumCert
- [x] HotStuff BFT consensus engine
- [x] Mempool with fee prioritization
- [x] World state + transaction execution
- [x] Ed25519 signing + verification
- [x] P2P networking — libp2p, Gossipsub, mDNS
- [x] HTTP API — Axum-based JSON endpoints
- [x] Built-in block explorer
- [x] Web wallet (Ed25519 key generation + signing)
- [x] Testnet faucet with cooldowns
- [x] Persistent block storage (sled)
- [x] Nonce enforcement + balance validation
- [x] Mempool deduplication
- [x] Chain sync protocol
- [x] Bootstrap peer support
- [ ] Smart contracts / programmable transactions
- [ ] Validator staking and rewards
- [ ] Expanded validator set
- [ ] Mainnet launch

---

## Built By

**Gaurav Sharma** — [GitHub @OrDex78](https://github.com/OrDex78) · [Twitter @gauravshar64966](https://twitter.com/gauravshar64966)

---

<div align="center">
<sub>No forks. No shortcuts. Just Rust.</sub>
</div>
