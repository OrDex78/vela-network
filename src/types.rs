// src/types.rs
// Core data types for Vela Network

use blake3::Hasher;
use chrono::Utc;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::fmt;

// ── Hash ─────────────────────────────────────────────────────────────────────

/// 32-byte Blake3 hash — used for blocks, transactions, everything
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    pub const ZERO: Hash = Hash([0u8; 32]);

    pub fn of(data: &[u8]) -> Self {
        let h = blake3::hash(data);
        Hash(*h.as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> anyhow::Result<Self> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            anyhow::bail!("invalid hash length");
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Hash(arr))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.to_hex()[..16])  // show first 16 chars
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", &self.to_hex()[..16])
    }
}

// ── Address ──────────────────────────────────────────────────────────────────

/// 32-byte public key used as address (like Solana, not ETH-style 20 bytes)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address([u8; 32]);

impl Address {
    pub fn from_pubkey(pk: &VerifyingKey) -> Self {
        Address(pk.to_bytes())
    }

    pub fn to_hex(&self) -> String {
        format!("vela:{}", hex::encode(self.0))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vela:{}", &hex::encode(self.0)[..16])
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

// ── Transaction ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub from:      Address,
    pub to:        Address,
    pub amount:    u64,        // in nVELA (1 VELA = 1_000_000_000 nVELA)
    pub fee:       u64,
    pub nonce:     u64,        // prevents replay attacks
    pub timestamp: i64,
    pub data:      Vec<u8>,    // optional payload (smart contract calls later)
    pub signature: Option<Vec<u8>>,
}

impl Transaction {
    pub fn new(from: Address, to: Address, amount: u64, fee: u64, nonce: u64) -> Self {
        Transaction {
            from,
            to,
            amount,
            fee,
            nonce,
            timestamp: Utc::now().timestamp_millis(),
            data: vec![],
            signature: None,
        }
    }

    /// Bytes to sign — everything except the signature itself
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut h = Hasher::new();
        h.update(self.from.as_bytes());
        h.update(self.to.as_bytes());
        h.update(&self.amount.to_le_bytes());
        h.update(&self.fee.to_le_bytes());
        h.update(&self.nonce.to_le_bytes());
        h.update(&self.timestamp.to_le_bytes());
        h.update(&self.data);
        h.finalize().as_bytes().to_vec()
    }

    pub fn sign(&mut self, key: &SigningKey) {
        use ed25519_dalek::Signer;
        let bytes = self.signing_bytes();
        let sig: Signature = key.sign(&bytes);
        self.signature = Some(sig.to_bytes().to_vec());
    }

    pub fn verify(&self) -> bool {
        use ed25519_dalek::Verifier;
        let Some(ref sig_bytes) = self.signature else { return false };
        let Ok(sig) = Signature::from_slice(sig_bytes) else { return false };
        let Ok(pk) = VerifyingKey::from_bytes(self.from.as_bytes()) else { return false };
        let bytes = self.signing_bytes();
        pk.verify(&bytes, &sig).is_ok()
    }

    pub fn hash(&self) -> Hash {
        Hash::of(&self.signing_bytes())
    }
}

// ── Block Header ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockHeader {
    pub height:        u64,
    pub parent_hash:   Hash,
    pub tx_root:       Hash,    // Merkle root of transactions
    pub state_root:    Hash,    // World state root
    pub timestamp:     i64,
    pub proposer:      Address, // validator who proposed this block
    pub round:         u64,     // HotStuff consensus round
}

impl BlockHeader {
    pub fn hash(&self) -> Hash {
        let bytes = bincode::serialize(self).unwrap_or_default();
        Hash::of(&bytes)
    }
}

// ── Block ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub header:       BlockHeader,
    pub transactions: Vec<Transaction>,
    pub qc:           Option<QuorumCert>, // HotStuff QC from previous round
}

impl Block {
    pub fn new(
        height: u64,
        parent_hash: Hash,
        transactions: Vec<Transaction>,
        proposer: Address,
        round: u64,
    ) -> Self {
        let tx_root = merkle_root(&transactions);
        Block {
            header: BlockHeader {
                height,
                parent_hash,
                tx_root,
                state_root: Hash::ZERO, // set after execution
                timestamp: Utc::now().timestamp_millis(),
                proposer,
                round,
            },
            transactions,
            qc: None,
        }
    }

    pub fn hash(&self) -> Hash {
        self.header.hash()
    }

    pub fn genesis() -> Self {
        Block {
            header: BlockHeader {
                height: 0,
                parent_hash: Hash::ZERO,
                tx_root: Hash::ZERO,
                state_root: Hash::ZERO,
                timestamp: 1_700_000_000_000, // fixed genesis time
                proposer: Address([0u8; 32]),
                round: 0,
            },
            transactions: vec![],
            qc: None,
        }
    }
}

// ── HotStuff Quorum Certificate ────────────────────────────────────────────────

/// A QC proves that 2f+1 validators signed a vote for a block
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuorumCert {
    pub block_hash: Hash,
    pub round:      u64,
    pub votes:      Vec<Vote>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vote {
    pub block_hash: Hash,
    pub round:      u64,
    pub voter:      Address,
    pub signature:  Vec<u8>,
}

impl Vote {
    pub fn signing_bytes(block_hash: &Hash, round: u64) -> Vec<u8> {
        let mut h = Hasher::new();
        h.update(block_hash.as_bytes());
        h.update(&round.to_le_bytes());
        h.finalize().as_bytes().to_vec()
    }

    pub fn sign(block_hash: Hash, round: u64, key: &SigningKey) -> Self {
        use ed25519_dalek::Signer;
        let bytes = Self::signing_bytes(&block_hash, round);
        let sig: Signature = key.sign(&bytes);
        Vote {
            block_hash,
            round,
            voter: Address::from_pubkey(&key.verifying_key()),
            signature: sig.to_bytes().to_vec(),
        }
    }

    pub fn verify(&self) -> bool {
        use ed25519_dalek::Verifier;
        let Ok(sig) = Signature::from_slice(&self.signature) else { return false };
        let Ok(pk) = VerifyingKey::from_bytes(self.voter.as_bytes()) else { return false };
        let bytes = Self::signing_bytes(&self.block_hash, self.round);
        pk.verify(&bytes, &sig).is_ok()
    }
}

// ── Merkle Root ───────────────────────────────────────────────────────────────

pub fn merkle_root(txs: &[Transaction]) -> Hash {
    if txs.is_empty() {
        return Hash::ZERO;
    }
    let mut hashes: Vec<Hash> = txs.iter().map(|tx| tx.hash()).collect();
    while hashes.len() > 1 {
        if hashes.len() % 2 == 1 {
            hashes.push(*hashes.last().unwrap()); // duplicate last if odd
        }
        hashes = hashes
            .chunks(2)
            .map(|pair| {
                let mut h = Hasher::new();
                h.update(pair[0].as_bytes());
                h.update(pair[1].as_bytes());
                Hash(*h.finalize().as_bytes())
            })
            .collect();
    }
    hashes[0]
}

// ── Account State ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub address: Address,
    pub balance: u64,
    pub nonce:   u64,
}

impl Account {
    pub fn new(address: Address) -> Self {
        Account { address, balance: 0, nonce: 0 }
    }
}

// ── Validator ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Validator {
    pub address: Address,
    pub stake:   u64,
    pub index:   usize, // position in validator set
}
