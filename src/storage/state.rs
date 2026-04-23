use crate::types::*;
use std::collections::HashMap;
use anyhow::Result;

pub struct WorldState { accounts: HashMap<String, Account> }

impl WorldState {
    pub fn new() -> Self { WorldState { accounts: HashMap::new() } }
    pub fn genesis(validators: &[Validator]) -> Self {
        let mut s = WorldState::new();
        for v in validators {
            s.accounts.insert(hex::encode(v.address.as_bytes()),
                Account { address: v.address, balance: 1_000_000_000_000_000, nonce: 0 });
        }
        s
    }
    pub fn apply_block(&mut self, block: &Block) -> Result<Hash> { Ok(block.hash()) }
    pub fn balance(&self, addr: &Address) -> u64 {
        self.accounts.get(&hex::encode(addr.as_bytes())).map_or(0, |a| a.balance)
    }
}