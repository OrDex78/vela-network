use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;

use crate::types::{Block, Hash};

// ── Shared node state ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct NodeState {
    pub port: u16,
    pub peer_count: Arc<RwLock<usize>>,
    pub blocks: Arc<RwLock<Vec<Block>>>,
}

impl NodeState {
    pub fn new(port: u16) -> Self {
        NodeState {
            port,
            peer_count: Arc::new(RwLock::new(0)),
            blocks: Arc::new(RwLock::new(vec![Block::genesis()])),
        }
    }
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    node: &'static str,
    version: &'static str,
    port: u16,
    peer_count: usize,
    block_height: u64,
}

#[derive(Serialize)]
struct BlockResponse {
    height: u64,
    hash: String,
    parent_hash: String,
    tx_count: usize,
    timestamp: i64,
    proposer: String,
    round: u64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn get_status(State(state): State<NodeState>) -> Json<StatusResponse> {
    let peer_count = *state.peer_count.read().await;
    let blocks = state.blocks.read().await;
    let height = blocks.last().map(|b| b.header.height).unwrap_or(0);

    Json(StatusResponse {
        node: "vela-node",
        version: "0.1.0",
        port: state.port,
        peer_count,
        block_height: height,
    })
}

async fn get_block(
    State(state): State<NodeState>,
    Path(height): Path<u64>,
) -> Result<Json<BlockResponse>, (StatusCode, Json<ErrorResponse>)> {
    let blocks = state.blocks.read().await;
    let block = blocks.iter().find(|b| b.header.height == height);

    match block {
        Some(b) => Ok(Json(BlockResponse {
            height: b.header.height,
            hash: b.hash().to_hex(),
            parent_hash: b.header.parent_hash.to_hex(),
            tx_count: b.transactions.len(),
            timestamp: b.header.timestamp,
            proposer: b.header.proposer.to_hex(),
            round: b.header.round,
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Block {} not found", height),
            }),
        )),
    }
}

async fn get_balance(
    State(state): State<NodeState>,
    Path(address): Path<String>,
) -> Json<serde_json::Value> {
    // Stub — Day 5 will wire up real world state
    Json(serde_json::json!({
        "address": address,
        "balance": 0,
        "nonce": 0,
        "note": "balance tracking coming in Day 5"
    }))
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn make_router(state: NodeState) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/block/{height}", get(get_block))
        .route("/balance/{address}", get(get_balance))
        .with_state(state)
}

pub async fn start_rpc(state: NodeState, rpc_port: u16) {
    let app = make_router(state);
    let addr = format!("0.0.0.0:{}", rpc_port);
    info!("RPC server listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}