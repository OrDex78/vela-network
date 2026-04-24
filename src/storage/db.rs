use crate::types::Block;
use anyhow::Result;
use sled::Db;

pub struct BlockDb {
    db: Db,
}

impl BlockDb {
    pub fn open(path: &str) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(BlockDb { db })
    }

    pub fn save_block(&self, block: &Block) -> Result<()> {
        let key = block.header.height.to_be_bytes();
        let val = bincode::serialize(block)?;
        self.db.insert(key, val)?;
        Ok(())
    }

    pub fn load_all_blocks(&self) -> Result<Vec<Block>> {
        let mut blocks = vec![];
        for item in self.db.iter() {
            let (_, val) = item?;
            let block: Block = bincode::deserialize(&val)?;
            blocks.push(block);
        }
        blocks.sort_by_key(|b| b.header.height);
        Ok(blocks)
    }

    pub fn height(&self) -> u64 {
        self.db.len() as u64
    }
}