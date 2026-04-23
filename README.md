# ⬡ Vela Network

> A custom blockchain built from scratch in Rust — HotStuff BFT consensus, Ed25519 signatures, Blake3 hashing.

Built by [Gaurav Sharma](https://github.com/OrDex78) | [@gauravshar64966](https://x.com/gauravshar64966)

---

## What is Vela?

Vela is a custom Layer-1 blockchain built entirely from scratch in Rust. No EVM fork, no Cosmos SDK — everything written by hand.

**Token:** VELA (1 VELA = 1,000,000,000 nVELA)  
**Consensus:** HotStuff BFT (powers Aptos, was used in Facebook's Diem)  
**Signatures:** Ed25519  
**Hashing:** Blake3  
**Addresses:** `vela:` prefix, 32-byte public key format

---

## Architecture

```
vela/
├── src/
│   ├── types.rs          # Block, Transaction, Hash, Vote, QuorumCert
│   ├── consensus/
│   │   └── hotstuff.rs   # HotStuff BFT — 3-phase voting, QC formation
│   ├── mempool/          # Transaction pool with fee prioritization
│   ├── crypto/           # Keypair generation, wallet files
│   ├── storage/
│   │   └── state.rs      # World state, balance tracking, tx execution
│   ├── p2p/              # libp2p networking (coming soon)
│   ├── rpc/              # JSON-RPC API (coming soon)
│   └── main.rs           # Node entry point
```

---

## HotStuff BFT

HotStuff achieves **linear message complexity** — O(n) messages per round, not O(n²) like classic PBFT. This makes it fast even with many validators.

**3 phases per block:**
1. **PREPARE** — leader proposes, replicas vote
2. **PRE-COMMIT** — leader forms prepareQC, replicas vote (and lock)  
3. **COMMIT** — leader forms commitQC → block finalized

A block requires **2f+1 votes** in each phase (where f = max Byzantine faults).  
With 4 validators: f=1, quorum=3.

---

## Run

```bash
# Clone
git clone https://github.com/OrDex78/vela-network
cd vela-network

# Run node (simulates consensus locally)
cargo run --bin vela-node

# Expected output:
# 🔷 Vela Network Node starting...
# ✅ Validator set: 4 nodes (f=1, quorum=3)
# 📦 Leader proposing block
# 🏆 PrepareQC formed
# 🏆 PreCommitQC formed  
# 🏆 CommitQC formed — BLOCK FINALIZED
# ✅ BLOCK COMMITTED SUCCESSFULLY
```

---

## Roadmap

- [x] Core types — Block, Transaction, Hash, Vote, QC
- [x] HotStuff BFT consensus engine
- [x] Mempool with fee prioritization
- [x] World state + transaction execution
- [x] Ed25519 signing + verification
- [ ] P2P networking (libp2p)
- [ ] JSON-RPC API
- [ ] Block explorer
- [ ] Multi-node testnet
- [ ] CLI wallet

---

## Why HotStuff?

Most tutorial blockchains use Nakamoto consensus (PoW) or simple PoA. HotStuff is the consensus algorithm behind **Aptos** (formerly Diem/Facebook). Building it from scratch demonstrates deep understanding of distributed systems, Byzantine fault tolerance, and cryptographic voting — not just "I forked geth."

---

*Part of a series of embedded + blockchain projects. Also built: [ESP32 Vault](https://github.com/OrDex78/esp32-vault) — hardware crypto wallet, [PhoneCanvas](https://github.com/OrDex78/phonecanvas) — wireless BLE display.*
