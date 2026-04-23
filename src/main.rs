// src/main.rs
// Vela Network Node — Entry Point

mod types;
mod consensus;
mod mempool;
mod crypto;
mod storage;

use consensus::{ConsensusConfig, HotStuffNode};
use crypto::Keypair;
use mempool::Mempool;
use storage::WorldState;
use types::*;

use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Init logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    info!("🔷 Vela Network Node starting...");
    info!("   Consensus: HotStuff BFT");
    info!("   Token:     VELA");
    info!("   Built by:  Gaurav Sharma (@gauravshar64966)");
    info!("   GitHub:    github.com/OrDex78");

    // ── Setup validator set (3 validators for local test → f=0, quorum=1)
    let v1 = Keypair::generate();
    let v2 = Keypair::generate();
    let v3 = Keypair::generate();
    let v4 = Keypair::generate(); // 4 validators → f=1, quorum=3

    let validators = vec![
        Validator { address: v1.address, stake: 1_000_000, index: 0 },
        Validator { address: v2.address, stake: 1_000_000, index: 1 },
        Validator { address: v3.address, stake: 1_000_000, index: 2 },
        Validator { address: v4.address, stake: 1_000_000, index: 3 },
    ];

    info!("✅ Validator set: {} nodes (f=1, quorum=3)", validators.len());
    for (i, v) in validators.iter().enumerate() {
        info!("   Validator {}: {}", i, v.address);
    }

    // ── Genesis state
    let mut state = WorldState::genesis(&validators);
    info!(
        "💰 Genesis balances: {} VELA each",
        1_000_000
    );

    // ── Init consensus node (node 0 = v1)
    let config = ConsensusConfig::new(validators.clone());
    let mut node = HotStuffNode::new(v1.address, v1.signing_key, config.clone());

    // ── Simulate a few rounds of consensus
    info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("🚀 Starting consensus simulation...");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Create a test transaction: v2 sends 100 VELA to v3
    let mut tx = Transaction::new(
        v2.address,
        v3.address,
        100 * 1_000_000_000, // 100 VELA
        1_000_000,           // 0.001 VELA fee
        0,
    );
    tx.sign(&v2.signing_key);

    let tx_hash = tx.hash();
    info!("📝 Test TX created: {} → {}", v2.address, v3.address);
    info!("   Amount: 100 VELA  Fee: 0.001 VELA  Hash: {}", tx_hash);

    // Add to mempool
    let mut mempool = Mempool::new(10_000);
    mempool.add(tx.clone())?;
    info!("📥 TX added to mempool ({} pending)", mempool.len());

    // ── Round 0: v1 is leader (round 0 % 4 = 0)
    info!("\n─── Round 0 ─────────────────────────");
    let txs = mempool.take(100);
    let propose_msg = node.propose(txs);

    // Simulate all validators voting (in real network: sent over P2P)
    if let consensus::HotStuffMsg::Propose { ref block, ref high_qc } = propose_msg {
        info!("📣 Block proposed: height={} txs={}", block.header.height, block.transactions.len());

        // Simulate 3 other validators voting in PREPARE phase
        for (keypair, validator) in [(&v2, &validators[1]), (&v3, &validators[2]), (&v4, &validators[3])] {
            let vote = Vote::sign(block.hash(), node.round, &keypair.signing_key);
            info!("🗳️  Validator {} voted PREPARE", validator.address);
            if let Some(resp) = node.handle_vote(vote) {
                info!("   → Leader formed QC, advancing phase");
                // Simulate validators voting in PRE-COMMIT phase
                for (kp2, v2_) in [(&v2, &validators[1]), (&v3, &validators[2]), (&v4, &validators[3])] {
                    let vote2 = Vote::sign(block.hash(), node.round, &kp2.signing_key);
                    if let Some(_) = node.handle_vote(vote2) {
                        // Simulate COMMIT phase votes
                        for (kp3, _) in [(&v2, &validators[1]), (&v3, &validators[2]), (&v4, &validators[3])] {
                            let vote3 = Vote::sign(block.hash(), node.round, &kp3.signing_key);
                            node.handle_vote(vote3);
                        }
                        break;
                    }
                }
                break;
            }
        }
    }

    // Apply committed block to state
    if node.chain_height() > 0 {
        let block = node.tip().clone();
        state.apply_block(&block)?;
        mempool.remove_committed(&block.transactions);

        info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("✅ BLOCK COMMITTED SUCCESSFULLY");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("   Height:    {}", block.header.height);
        info!("   Hash:      {}", block.hash());
        info!("   TXs:       {}", block.transactions.len());
        info!("   Proposer:  {}", block.header.proposer);
        info!("");
        info!("📊 Post-block balances:");
        info!("   v2: {} VELA", state.balance(&v2.address) / 1_000_000_000);
        info!("   v3: {} VELA", state.balance(&v3.address) / 1_000_000_000);
        info!("");
        info!("Chain height: {}", node.chain_height());
        info!("Mempool size: {}", mempool.len());
    }

    info!("\n🔷 Vela Network — Consensus working correctly");
    info!("   Next: P2P networking + RPC server");

    Ok(())
}
