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
use types::{Block, BlockHeader, Address, Hash};

#[derive(Parser, Debug)]
#[command(name = "vela-node", about = "Vela Network Node")]
struct Args {
    #[arg(long, default_value = "8001")]
    port: u16,

    #[arg(long, value_delimiter = ',')]
    bootstrap: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let args = Args::parse();
    let rpc_port = args.port + 1000; // 8001 → 9001, 8002 → 9002, etc.

    info!("Starting Vela node on port {}", args.port);
    info!("RPC API on port {}", rpc_port);

    let bootstrap_peers: Vec<Multiaddr> = args
        .bootstrap
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    // ── Shared state ──────────────────────────────────────────────────────────
    let node_state = NodeState::new(args.port);

    // ── P2P ───────────────────────────────────────────────────────────────────
    let (tx_in, mut rx_in) = mpsc::channel::<NetworkMessage>(256);
    let node = P2PNode::new(args.port, bootstrap_peers, tx_in)?;
    let tx_out = node.tx_out.clone();

    tokio::spawn(async move {
        if let Err(e) = node.run().await {
            tracing::error!("P2P node error: {e}");
        }
    });

    // ── Incoming message handler ──────────────────────────────────────────────
    let state_for_p2p = node_state.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx_in.recv().await {
            match msg {
                NetworkMessage::NewBlock(block) => {
                    info!("📦 Received block #{} from network", block.header.height);
                    state_for_p2p.blocks.write().await.push(block);
                }
                NetworkMessage::NewTransaction(tx) => {
                    info!("💸 Received tx {} from network", tx.hash().to_hex());
                }
            }
        }
    });

    // ── Broadcast dummy block after 3s ────────────────────────────────────────
    let tx_out_clone = tx_out.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let dummy_block = Block {
            header: BlockHeader {
                height: 1,
                parent_hash: Hash::ZERO,
                tx_root: Hash::ZERO,
                state_root: Hash::ZERO,
                timestamp: chrono::Utc::now().timestamp_millis(),
                proposer: Address([0u8; 32]),
                round: 1,
            },
            transactions: vec![],
            qc: None,
        };
        info!("📡 Broadcasting dummy block to network...");
        tx_out_clone.send(NetworkMessage::NewBlock(dummy_block)).await.ok();
    });

    // ── RPC server ────────────────────────────────────────────────────────────
    let state_for_rpc = node_state.clone();
    tokio::spawn(async move {
        start_rpc(state_for_rpc, rpc_port).await;
    });

    tokio::signal::ctrl_c().await?;
    info!("Shutting down.");
    Ok(())
}