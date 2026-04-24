use std::sync::Arc;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tracing::info;
use crate::types::{Address, Block, Transaction};

#[derive(Clone)]
pub struct NodeState {
    pub port: u16,
    pub peer_count: Arc<RwLock<usize>>,
    pub blocks: Arc<RwLock<Vec<Block>>>,
    pub mempool: Arc<RwLock<Vec<Transaction>>>,
    pub tx_broadcast: mpsc::Sender<Transaction>,
}

impl NodeState {
    pub fn new(port: u16, tx_broadcast: mpsc::Sender<Transaction>) -> Self {
        NodeState {
            port,
            peer_count: Arc::new(RwLock::new(0)),
            blocks: Arc::new(RwLock::new(vec![Block::genesis()])),
            mempool: Arc::new(RwLock::new(vec![])),
            tx_broadcast,
        }
    }
}

#[derive(Serialize)]
struct StatusResponse {
    node: &'static str,
    version: &'static str,
    port: u16,
    peer_count: usize,
    block_height: u64,
    mempool_size: usize,
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

#[derive(Deserialize)]
pub struct SendTxRequest {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
}

#[derive(Serialize)]
struct TxResponse {
    status: String,
    tx_hash: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

async fn get_status(State(state): State<NodeState>) -> Json<StatusResponse> {
    let peer_count = *state.peer_count.read().await;
    let blocks = state.blocks.read().await;
    let mempool = state.mempool.read().await;
    let height = blocks.last().map(|b| b.header.height).unwrap_or(0);
    Json(StatusResponse {
        node: "vela-node",
        version: "0.1.0",
        port: state.port,
        peer_count,
        block_height: height,
        mempool_size: mempool.len(),
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
            Json(ErrorResponse { error: format!("Block {} not found", height) }),
        )),
    }
}

async fn get_balance(
    State(_state): State<NodeState>,
    Path(address): Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "address": address,
        "balance": 0,
        "nonce": 0
    }))
}

async fn post_send_tx(
    State(state): State<NodeState>,
    Json(req): Json<SendTxRequest>,
) -> Result<Json<TxResponse>, (StatusCode, Json<ErrorResponse>)> {
    let from_bytes = hex::decode(req.from.trim_start_matches("vela:"))
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid from".into() })))?;
    let to_bytes = hex::decode(req.to.trim_start_matches("vela:"))
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid to".into() })))?;
    if from_bytes.len() != 32 || to_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "address must be 32 bytes".into() })));
    }
    let mut from_arr = [0u8; 32];
    let mut to_arr = [0u8; 32];
    from_arr.copy_from_slice(&from_bytes);
    to_arr.copy_from_slice(&to_bytes);
    let tx = Transaction::new(Address(from_arr), Address(to_arr), req.amount, req.fee, req.nonce);
    let tx_hash = tx.hash().to_hex();
    state.mempool.write().await.push(tx.clone());
    info!("💸 TX added to mempool: {}", &tx_hash[..16]);
    state.tx_broadcast.send(tx).await.ok();
    Ok(Json(TxResponse { status: "accepted".into(), tx_hash }))
}

async fn get_explorer() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("explorer.html"))
}

pub fn make_router(state: NodeState) -> Router {
    Router::new()
        .route("/", get(get_explorer))
        .route("/status", get(get_status))
        .route("/block/{height}", get(get_block))
        .route("/balance/{address}", get(get_balance))
        .route("/send_tx", post(post_send_tx))
        .with_state(state)
}

pub async fn start_rpc(state: NodeState, rpc_port: u16) {
    use tower_http::cors::{Any, CorsLayer};
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = make_router(state).layer(cors);
    let addr = format!("0.0.0.0:{}", rpc_port);
    info!("RPC server listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}