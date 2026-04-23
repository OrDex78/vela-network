// src/consensus/hotstuff.rs
// HotStuff BFT Consensus — linear message complexity, 3-phase voting
//
// Phases per round:
//   PREPARE  → leader proposes block, replicas vote
//   PRE-COMMIT → leader collects votes → prepareQC, sends to replicas
//   COMMIT   → replicas vote on prepareQC → precommitQC
//   DECIDE   → leader collects → commitQC → block finalized
//
// Safety: a block is committed only when 2f+1 validators voted in all 3 phases
// Liveness: view-change (pacemaker) replaces slow/faulty leader

use crate::types::*;
use anyhow::Result;
use ed25519_dalek::SigningKey;
use std::collections::HashMap;
use tracing::{info, warn};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ConsensusConfig {
    pub validator_set:  Vec<Validator>,
    pub f:              usize,    // max faulty nodes (n >= 3f+1)
    pub block_interval: u64,      // ms between blocks
    pub timeout_ms:     u64,      // round timeout before view change
}

impl ConsensusConfig {
    pub fn new(validators: Vec<Validator>) -> Self {
        let n = validators.len();
        let f = (n - 1) / 3; // max Byzantine faults
        ConsensusConfig {
            validator_set: validators,
            f,
            block_interval: 1000,
            timeout_ms: 5000,
        }
    }

    pub fn quorum_size(&self) -> usize {
        2 * self.f + 1 // 2f+1 votes needed
    }

    pub fn is_leader(&self, round: u64, address: &Address) -> bool {
        let idx = (round as usize) % self.validator_set.len();
        &self.validator_set[idx].address == address
    }

    pub fn leader_for_round(&self, round: u64) -> &Validator {
        let idx = (round as usize) % self.validator_set.len();
        &self.validator_set[idx]
    }
}

// ── Phase ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Prepare,
    PreCommit,
    Commit,
    Decide,
}

// ── HotStuff Messages ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum HotStuffMsg {
    /// Leader → Replicas: here's the new block proposal
    Propose {
        block:      Block,
        high_qc:    Option<QuorumCert>,
    },
    /// Replica → Leader: I vote for this block
    Vote(Vote),
    /// Leader → Replicas: we have a QC, move to next phase
    NewView {
        round:   u64,
        high_qc: QuorumCert,
    },
    /// Timeout: replica gives up on current leader
    Timeout {
        round:   u64,
        voter:   Address,
    },
}

// ── Node State ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct HotStuffNode {
    pub address:      Address,
    pub signing_key:  SigningKey,
    pub config:       ConsensusConfig,

    // Current round state
    pub round:        u64,
    pub phase:        Phase,
    pub locked_qc:    Option<QuorumCert>,  // highest QC we're locked on
    pub high_qc:      Option<QuorumCert>,  // highest QC we've seen

    // Pending votes for current round
    pub prepare_votes:    HashMap<String, Vote>, // voter_hex → vote
    pub precommit_votes:  HashMap<String, Vote>,
    pub commit_votes:     HashMap<String, Vote>,

    // Finalized chain
    pub committed_blocks: Vec<Block>,
    pub pending_block:    Option<Block>,
}

impl HotStuffNode {
    pub fn new(address: Address, signing_key: SigningKey, config: ConsensusConfig) -> Self {
        HotStuffNode {
            address,
            signing_key,
            config,
            round: 0,
            phase: Phase::Prepare,
            locked_qc: None,
            high_qc: None,
            prepare_votes: HashMap::new(),
            precommit_votes: HashMap::new(),
            commit_votes: HashMap::new(),
            committed_blocks: vec![Block::genesis()],
            pending_block: None,
        }
    }

    pub fn is_leader(&self) -> bool {
        self.config.is_leader(self.round, &self.address)
    }

    pub fn chain_height(&self) -> u64 {
        self.committed_blocks.len() as u64 - 1
    }

    pub fn tip(&self) -> &Block {
        self.committed_blocks.last().unwrap()
    }

    // ── Leader: propose a new block ───────────────────────────────────────────

    pub fn propose(&mut self, transactions: Vec<Transaction>) -> HotStuffMsg {
        assert!(self.is_leader(), "only leader can propose");

        let parent = self.tip();
        let block = Block::new(
            parent.header.height + 1,
            parent.hash(),
            transactions,
            self.address,
            self.round,
        );

        self.pending_block = Some(block.clone());
        self.phase = Phase::Prepare;

        info!(
            round = self.round,
            height = block.header.height,
            txs = block.transactions.len(),
            "📦 Leader proposing block"
        );

        HotStuffMsg::Propose {
            block,
            high_qc: self.high_qc.clone(),
        }
    }

    // ── Replica: handle a proposal ────────────────────────────────────────────

    pub fn handle_propose(&mut self, block: Block, _high_qc: Option<QuorumCert>) -> Option<HotStuffMsg> {
        // Safety rule: only vote if block extends our locked QC
        if let Some(ref locked) = self.locked_qc {
            if block.header.parent_hash != locked.block_hash {
                // Check if the new block's QC is higher than our lock
                // (allows progress if we were locked on a stale block)
                warn!(round = self.round, "Block doesn't extend locked QC, checking safety...");
                // For now: reject (full liveness requires more logic)
                return None;
            }
        }

        info!(
            round = self.round,
            height = block.header.height,
            "✅ Replica voting PREPARE for block {}", block.hash()
        );

        self.pending_block = Some(block.clone());

        let vote = Vote::sign(block.hash(), self.round, &self.signing_key);
        Some(HotStuffMsg::Vote(vote))
    }

    // ── Leader: collect votes and form QC ─────────────────────────────────────

    pub fn handle_vote(&mut self, vote: Vote) -> Option<HotStuffMsg> {
        if !vote.verify() {
            warn!("Invalid vote signature from {:?}", vote.voter);
            return None;
        }

        let voter_hex = hex::encode(vote.voter.as_bytes());

        match self.phase {
            Phase::Prepare => {
                self.prepare_votes.insert(voter_hex, vote);
                if self.prepare_votes.len() >= self.config.quorum_size() {
                    let qc = self.form_qc(Phase::Prepare);
                    info!(round = self.round, votes = self.prepare_votes.len(), "🏆 PrepareQC formed");
                    self.high_qc = Some(qc.clone());
                    self.phase = Phase::PreCommit;
                    return Some(HotStuffMsg::NewView { round: self.round, high_qc: qc });
                }
            }
            Phase::PreCommit => {
                self.precommit_votes.insert(voter_hex, vote);
                if self.precommit_votes.len() >= self.config.quorum_size() {
                    let qc = self.form_qc(Phase::PreCommit);
                    info!(round = self.round, "🏆 PreCommitQC formed");
                    self.locked_qc = Some(qc.clone()); // LOCK HERE
                    self.phase = Phase::Commit;
                    return Some(HotStuffMsg::NewView { round: self.round, high_qc: qc });
                }
            }
            Phase::Commit => {
                self.commit_votes.insert(voter_hex, vote);
                if self.commit_votes.len() >= self.config.quorum_size() {
                    let qc = self.form_qc(Phase::Commit);
                    info!(round = self.round, "🏆 CommitQC formed — BLOCK FINALIZED");
                    self.phase = Phase::Decide;
                    self.commit_block(qc.clone());
                    return Some(HotStuffMsg::NewView { round: self.round + 1, high_qc: qc });
                }
            }
            Phase::Decide => {}
        }

        None
    }

    // ── Replica: handle NewView (phase advance) ───────────────────────────────

    pub fn handle_new_view(&mut self, round: u64, high_qc: QuorumCert) -> Option<HotStuffMsg> {
        if high_qc.round >= self.high_qc.as_ref().map_or(0, |q| q.round) {
            self.high_qc = Some(high_qc.clone());
        }

        // If this advances us to a new round → vote in next phase
        if round > self.round {
            self.advance_round(round);
            return None;
        }

        // Vote for current pending block in the new phase
        if let Some(ref block) = self.pending_block.clone() {
            let vote = Vote::sign(block.hash(), self.round, &self.signing_key);
            return Some(HotStuffMsg::Vote(vote));
        }

        None
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn form_qc(&self, phase: Phase) -> QuorumCert {
        let votes: Vec<Vote> = match phase {
            Phase::Prepare    => self.prepare_votes.values().cloned().collect(),
            Phase::PreCommit  => self.precommit_votes.values().cloned().collect(),
            Phase::Commit     => self.commit_votes.values().cloned().collect(),
            Phase::Decide     => vec![],
        };
        let block_hash = votes.first().map(|v| v.block_hash).unwrap_or(Hash::ZERO);
        QuorumCert { block_hash, round: self.round, votes }
    }

    fn commit_block(&mut self, qc: QuorumCert) {
        if let Some(mut block) = self.pending_block.take() {
            block.qc = Some(qc);
            let height = block.header.height;
            let hash = block.hash();
            self.committed_blocks.push(block);
            self.prepare_votes.clear();
            self.precommit_votes.clear();
            self.commit_votes.clear();
            info!(height, hash = %hash, "⛓️  Block committed to chain");
        }
    }

    fn advance_round(&mut self, new_round: u64) {
        self.round = new_round;
        self.phase = Phase::Prepare;
        self.pending_block = None;
        self.prepare_votes.clear();
        self.precommit_votes.clear();
        self.commit_votes.clear();
        info!(round = self.round, "🔄 Advanced to new round");
    }
}
