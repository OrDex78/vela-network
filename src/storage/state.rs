use crate::types::*;
use std::collections::HashMap;
use anyhow::Result;

pub const TOTAL_SUPPLY: u64 = 1_000_000_000; // 1B VELA
pub const FAUCET_AMOUNT: u64 = 100;
pub const FAUCET_COOLDOWN_SECS: u64 = 86400; // 24 hours

// Faucet address — validator key index 0 with seed [1,0,0...]
// funded at genesis with 100M VELA
pub const FAUCET_SEED: [u8; 32] = [
    0xFA, 0xCE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

pub struct WorldState {
    pub accounts: HashMap<String, Account>,
    pub total_supply: u64,
    pub total_txs: u64,
    pub faucet_last: HashMap<String, u64>, // address -> unix timestamp
}

impl WorldState {
    pub fn new() -> Self {
        WorldState {
            accounts: HashMap::new(),
            total_supply: 0,
            total_txs: 0,
            faucet_last: HashMap::new(),
        }
    }

    pub fn apply_genesis(&mut self) {
        use ed25519_dalek::SigningKey;

        // Mint 1B VELA to treasury (validator 0)
        let treasury_key = SigningKey::from_bytes(&[1u8; 32]);
        let treasury_addr = Address::from_pubkey(&treasury_key.verifying_key());
        let treasury_hex = hex::encode(treasury_addr.as_bytes());

        // Mint 100M to faucet
        let faucet_key = SigningKey::from_bytes(&FAUCET_SEED);
        let faucet_addr = Address::from_pubkey(&faucet_key.verifying_key());
        let faucet_hex = hex::encode(faucet_addr.as_bytes());

        self.accounts.insert(treasury_hex, Account {
            address: treasury_addr,
            balance: 900_000_000,
            nonce: 0,
        });

        self.accounts.insert(faucet_hex, Account {
            address: faucet_addr,
            balance: 100_000_000,
            nonce: 0,
        });

        self.total_supply = TOTAL_SUPPLY;
    }

    pub fn apply_block(&mut self, block: &Block) -> Result<()> {
        // Apply genesis mint on block 0
        if block.header.height == 0 && self.total_supply == 0 {
            self.apply_genesis();
        }

        for tx in &block.transactions {
            let from_key = hex::encode(tx.from.as_bytes());
            let to_key = hex::encode(tx.to.as_bytes());

            let from_balance = self.accounts.get(&from_key)
                .map_or(0, |a| a.balance);

            if from_balance >= tx.amount + tx.fee {
    self.accounts.entry(from_key.clone()).or_insert(Account {
        address: tx.from,
        balance: 0,
        nonce: 0,
    }).balance -= tx.amount + tx.fee;

    self.accounts.entry(from_key.clone()).and_modify(|a| a.nonce += 1);

                self.accounts.entry(to_key).or_insert(Account {
                    address: tx.to,
                    balance: 0,
                    nonce: 0,
                }).balance += tx.amount;

                self.total_txs += 1;
            }
        }
        Ok(())
    }

    pub fn balance(&self, addr: &Address) -> u64 {
        self.accounts
            .get(&hex::encode(addr.as_bytes()))
            .map_or(0, |a| a.balance)
    }

    pub fn nonce(&self, addr: &Address) -> u64 {
        self.accounts
            .get(&hex::encode(addr.as_bytes()))
            .map_or(0, |a| a.nonce)
    }

    pub fn can_faucet(&self, addr: &str) -> bool {
        match self.faucet_last.get(addr) {
            None => true,
            Some(&last) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now - last >= FAUCET_COOLDOWN_SECS
            }
        }
    }

    pub fn mark_faucet(&mut self, addr: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.faucet_last.insert(addr.to_string(), now);
    }
}