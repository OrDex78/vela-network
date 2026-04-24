mod consensus;
mod crypto;
mod mempool;
mod network;
mod rpc;
mod storage;
mod types;

use anyhow::Result;
use clap::Parser;
use ed25519_dalek::SigningKey;
use libp2p::Multiaddr;
use rand::rngs::OsRng;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::info;

use consensus::hotstuff::{ConsensusConfig, HotStuffNode};
use network::{NetworkMessage, P2PNode};
use rpc::{NodeState, start_rpc};
use storage::db::BlockDb;
use storage::state::WorldState;
use types::{Address, Block, BlockHeader, Hash, Transaction, Validator, Vote};

#[derive(Parser, Debug)]
#[command(name = "vela-node", about = "Vela Network Node")]
struct Args {
    #[arg(long, default_value = "8001")]
    port: u16,

    #[arg(long, value_delimiter = ',')]
    bootstrap: Vec<String>,

    /// Validator index (0, 1, or 2)
    #[arg(long, default_value = "0")]
    validator_index: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let args = Args::parse();
    let rpc_port = args.port + 1000;

    info!("Starting Vela node on port {}", args.port);
    info!("RPC API on port {}", rpc_port);
    info!("Validator index: {}", args.validator_index);

    let bootstrap_peers: Vec<Multiaddr> = args
        .bootstrap
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    // ── Validator keypairs (deterministic from index for testnet) ─────────────
    // In production these would be loaded from disk
    let validator_keys: Vec<SigningKey> = (0..3).map(|i| {
        let mut seed = [0u8; 32];
        seed[0] = i as u8 + 1;
        SigningKey::from_bytes(&seed)
    }).collect();

    let validators: Vec<Validator> = validator_keys.iter().enumerate().map(|(i, key)| {
        Validator {
            address: Address::from_pubkey(&key.verifying_key()),
            stake: 1000,
            index: i,
        }
    }).collect();

    let my_key = validator_keys[args.validator_index].clone();
    let my_address = Address::from_pubkey(&my_key.verifying_key());
    info!("My validator address: {}", my_address);

    // ── Persistent storage ────────────────────────────────────────────────────
    let db_path = format!("vela-db-{}", args.port);
    let block_db = Arc::new(BlockDb::open(&db_path)?);

    let mut initial_blocks = block_db.load_all_blocks()?;
    if initial_blocks.is_empty() {
        let genesis = Block::genesis();
        block_db.save_block(&genesis)?;
        initial_blocks.push(genesis);
    }

    let mut initial_world_state = WorldState::new();
    for block in &initial_blocks {
        initial_world_state.apply_block(block).ok();
    }

    info!("Loaded {} blocks from disk", initial_blocks.len());

    // ── Consensus engine ──────────────────────────────────────────────────────
    let config = ConsensusConfig::new(validators.clone());
    let hotstuff = Arc::new(RwLock::new(
        HotStuffNode::new(my_address, my_key.clone(), config)
    ));

    let (tx_in, mut rx_in) = mpsc::channel::<NetworkMessage>(256);
    let (tx_broadcast, mut rx_broadcast) = mpsc::channel::<Transaction>(256);

    let node_state = NodeState::new_with_state(
        args.port,
        tx_broadcast,
        initial_blocks,
        initial_world_state,
    );

    let node = P2PNode::new(args.port, bootstrap_peers, tx_in)?;
    let tx_out = node.tx_out.clone();

    tokio::spawn(async move {
        if let Err(e) = node.run().await {
            tracing::error!("P2P node error: {e}");
        }
    });

    // ── Incoming P2P messages ─────────────────────────────────────────────────
    let state_p2p = node_state.clone();
    let db_p2p = block_db.clone();
    let hotstuff_p2p = hotstuff.clone();
    let tx_out_p2p = tx_out.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx_in.recv().await {
            match msg {
                NetworkMessage::NewBlock(block) => {
                    info!("📦 Received block #{} from network", block.header.height);
                    let mut blocks = state_p2p.blocks.write().await;
                    let tip = blocks.last().map(|b| b.header.height).unwrap_or(0);
                    if block.header.height == tip + 1 {
                        state_p2p.world_state.write().await.apply_block(&block).ok();
                        db_p2p.save_block(&block).ok();
                        blocks.push(block.clone());
                        info!("✅ Block committed, chain height: {}", tip + 1);

                        // Send vote for this block
                        let vote = {
                            let hs = hotstuff_p2p.read().await;
                            Vote::sign(block.hash(), block.header.round, &hs.signing_key)
                        };
                        tx_out_p2p.send(NetworkMessage::ConsensusVote(vote)).await.ok();
                    }
                }
                NetworkMessage::NewTransaction(tx) => {
                    info!("💸 Received tx from network");
                    state_p2p.mempool.write().await.push(tx);
                }
                NetworkMessage::ConsensusVote(vote) => {
                    info!("🗳️ Received vote from {:?}", vote.voter);
                    let result = {
                        let mut hs = hotstuff_p2p.write().await;
                        hs.handle_vote(vote)
                    };
                    if let Some(msg) = result {
                        info!("📨 Sending consensus message after vote");
                        let _ = tx_out_p2p.send(NetworkMessage::NewBlock(
                            // placeholder — real QC block commit handled below
                            Block::genesis()
                        )).await;
                    }
                }
                NetworkMessage::ConsensusPropose(block) => {
                    info!("📋 Received proposal for block #{}", block.header.height);
                    let result = {
                        let mut hs = hotstuff_p2p.write().await;
                        hs.handle_propose(block.clone(), None)
                    };
                    if let Some(_vote_msg) = result {
                        let vote = {
                            let hs = hotstuff_p2p.read().await;
                            Vote::sign(block.hash(), block.header.round, &hs.signing_key)
                        };
                        tx_out_p2p.send(NetworkMessage::ConsensusVote(vote)).await.ok();
                    }
                }
            }
        }
    });

    // ── Broadcast txs from RPC to network ────────────────────────────────────
    let tx_out_tx = tx_out.clone();
    tokio::spawn(async move {
        while let Some(tx) = rx_broadcast.recv().await {
            tx_out_tx.send(NetworkMessage::NewTransaction(tx)).await.ok();
        }
    });

    // ── Block production (leader only) ────────────────────────────────────────
    let state_prod = node_state.clone();
    let tx_out_prod = tx_out.clone();
    let db_prod = block_db.clone();
    let hotstuff_prod = hotstuff.clone();
    let validator_index = args.validator_index;
    tokio::spawn(async move {
        let mut round = state_prod.blocks.read().await.last()
            .map(|b| b.header.round + 1).unwrap_or(1);

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

            // Only produce block if we are the leader for this round
            let is_leader = round % 3 == validator_index as u64;

            if is_leader {
                let mut blocks = state_prod.blocks.write().await;
                let mut mempool = state_prod.mempool.write().await;

                let tip = blocks.last().unwrap();
                let parent_hash = tip.hash();
                let height = tip.header.height + 1;
                let txs: Vec<_> = mempool.drain(..).take(100).collect();

                let block = Block {
                    header: BlockHeader {
                        height,
                        parent_hash,
                        tx_root: types::merkle_root(&txs),
                        state_root: Hash::ZERO,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        proposer: Address([0u8; 32]),
                        round,
                    },
                    transactions: txs,
                    qc: None,
                };

                info!("⛏️  Leader producing block #{} with {} txs", height, block.transactions.len());
                state_prod.world_state.write().await.apply_block(&block).ok();
                db_prod.save_block(&block).ok();
                blocks.push(block.clone());
                drop(blocks);
                drop(mempool);

                // Broadcast proposal to all nodes
                tx_out_prod.send(NetworkMessage::ConsensusPropose(block.clone())).await.ok();
                tx_out_prod.send(NetworkMessage::NewBlock(block)).await.ok();
            } else {
                info!("⏳ Round {} — waiting for leader (validator {})", round, round % 3);
            }

            round += 1;
        }
    });

    // ── RPC server ────────────────────────────────────────────────────────────
    let state_rpc = node_state.clone();
    tokio::spawn(async move {
        start_rpc(state_rpc, rpc_port).await;
    });

    tokio::signal::ctrl_c().await?;
    info!("Shutting down.");
    Ok(())
}