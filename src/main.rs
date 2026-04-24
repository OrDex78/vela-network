mod consensus;
mod crypto;
mod mempool;
mod network;
mod rpc;
mod storage;
mod types;

use anyhow::Result;
use clap::Parser;
use libp2p::Multiaddr;
use tokio::sync::mpsc;
use tracing::info;

use network::{NetworkMessage, P2PNode};
use rpc::{NodeState, start_rpc};
use storage::db::BlockDb;
use storage::state::WorldState;
use types::{Address, Block, BlockHeader, Hash, Transaction};

#[derive(Parser, Debug)]
#[command(name = "vela-node", about = "Vela Network Node")]
struct Args {
    #[arg(long, default_value = "8001")]
    port: u16,

    #[arg(long, value_delimiter = ',')]
    bootstrap: Vec<String>,

    #[arg(long, default_value = "vela-db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let args = Args::parse();
    let rpc_port = args.port + 1000;

    info!("Starting Vela node on port {}", args.port);
    info!("RPC API on port {}", rpc_port);

    let bootstrap_peers: Vec<Multiaddr> = args
        .bootstrap
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    // ── Load or init persistent storage ──────────────────────────────────────
    let db_path = format!("{}-{}", args.db_path, args.port);
    let block_db = BlockDb::open(&db_path)?;

    let mut initial_blocks = block_db.load_all_blocks()?;
    if initial_blocks.is_empty() {
        let genesis = Block::genesis();
        block_db.save_block(&genesis)?;
        initial_blocks.push(genesis);
    }

    // Replay blocks to rebuild world state
    let mut initial_world_state = WorldState::new();
    for block in &initial_blocks {
        initial_world_state.apply_block(block).ok();
    }

    info!("Loaded {} blocks from disk", initial_blocks.len());

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

    let state_p2p = node_state.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx_in.recv().await {
            match msg {
                NetworkMessage::NewBlock(block) => {
                    info!("📦 Received block #{} from network", block.header.height);
                    let mut blocks = state_p2p.blocks.write().await;
                    let tip = blocks.last().map(|b| b.header.height).unwrap_or(0);
                    if block.header.height == tip + 1 {
                        state_p2p.world_state.write().await.apply_block(&block).ok();
                        blocks.push(block);
                        info!("✅ Block committed, chain height: {}", tip + 1);
                    }
                }
                NetworkMessage::NewTransaction(tx) => {
                    info!("💸 Received tx from network");
                    state_p2p.mempool.write().await.push(tx);
                }
            }
        }
    });

    let tx_out_tx = tx_out.clone();
    tokio::spawn(async move {
        while let Some(tx) = rx_broadcast.recv().await {
            tx_out_tx.send(NetworkMessage::NewTransaction(tx)).await.ok();
        }
    });

    let state_prod = node_state.clone();
    let tx_out_prod = tx_out.clone();
    let db_path2 = format!("{}-{}", args.db_path, args.port);
    tokio::spawn(async move {
        let block_db = BlockDb::open(&db_path2).expect("db open");
        let mut round = state_prod.blocks.read().await.last()
            .map(|b| b.header.round + 1).unwrap_or(1);

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

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

            info!("⛏️  Producing block #{} with {} txs", height, block.transactions.len());
            state_prod.world_state.write().await.apply_block(&block).ok();
            block_db.save_block(&block).ok();
            blocks.push(block.clone());
            drop(blocks);
            drop(mempool);

            tx_out_prod.send(NetworkMessage::NewBlock(block)).await.ok();
            round += 1;
        }
    });

    let state_rpc = node_state.clone();
    tokio::spawn(async move {
        start_rpc(state_rpc, rpc_port).await;
    });

    tokio::signal::ctrl_c().await?;
    info!("Shutting down.");
    Ok(())
}