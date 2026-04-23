use crate::types::*;
use std::collections::HashMap;

pub struct Mempool {
    transactions: HashMap<String, Transaction>,
}

impl Mempool {
    pub fn new(_max: usize) -> Self { Mempool { transactions: HashMap::new() } }
    pub fn add(&mut self, tx: Transaction) -> anyhow::Result<Hash> {
        let h = tx.hash();
        self.transactions.insert(h.to_hex(), tx);
        Ok(h)
    }
    pub fn take(&mut self, limit: usize) -> Vec<Transaction> {
        self.transactions.values().cloned().take(limit).collect()
    }
    pub fn remove_committed(&mut self, txs: &[Transaction]) {
        for tx in txs { self.transactions.remove(&tx.hash().to_hex()); }
    }
    pub fn len(&self) -> usize { self.transactions.len() }
}
