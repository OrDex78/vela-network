use crate::types::*;
use std::collections::HashMap;
use anyhow::Result;

pub struct WorldState {
    accounts: HashMap<String, Account>,
}

impl WorldState {
    pub fn new() -> Self {
        WorldState { accounts: HashMap::new() }
    }

    pub fn apply_block(&mut self, block: &Block) -> Result<()> {
        for tx in &block.transactions {
            let from_key = hex::encode(tx.from.as_bytes());
            let to_key = hex::encode(tx.to.as_bytes());

            // Deduct from sender
            let from_acc = self.accounts.entry(from_key).or_insert(Account {
                address: tx.from,
                balance: 0,
                nonce: 0,
            });
            if from_acc.balance >= tx.amount + tx.fee {
                from_acc.balance -= tx.amount + tx.fee;
                from_acc.nonce += 1;
            }

            // Credit receiver
            let to_acc = self.accounts.entry(to_key).or_insert(Account {
                address: tx.to,
                balance: 0,
                nonce: 0,
            });
            to_acc.balance += tx.amount;
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
}