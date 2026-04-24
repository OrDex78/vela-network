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
use crate::storage::state::{WorldState, FAUCET_SEED, FAUCET_AMOUNT};

#[derive(Clone)]
pub struct NodeState {
    pub port: u16,
    pub peer_count: Arc<RwLock<usize>>,
    pub blocks: Arc<RwLock<Vec<Block>>>,
    pub mempool: Arc<RwLock<Vec<Transaction>>>,
    pub tx_broadcast: mpsc::Sender<Transaction>,
    pub world_state: Arc<RwLock<WorldState>>,
    pub current_round: Arc<RwLock<u64>>,
}

impl NodeState {
    pub fn new(port: u16, tx_broadcast: mpsc::Sender<Transaction>) -> Self {
        NodeState {
            port,
            peer_count: Arc::new(RwLock::new(0)),
            blocks: Arc::new(RwLock::new(vec![Block::genesis()])),
            mempool: Arc::new(RwLock::new(vec![])),
            tx_broadcast,
            world_state: Arc::new(RwLock::new(WorldState::new())),
            current_round: Arc::new(RwLock::new(0)),
        }
    }

    pub fn new_with_state(
        port: u16,
        tx_broadcast: mpsc::Sender<Transaction>,
        blocks: Vec<Block>,
        world_state: WorldState,
    ) -> Self {
        NodeState {
            port,
            peer_count: Arc::new(RwLock::new(0)),
            blocks: Arc::new(RwLock::new(blocks)),
            mempool: Arc::new(RwLock::new(vec![])),
            tx_broadcast,
            world_state: Arc::new(RwLock::new(world_state)),
            current_round: Arc::new(RwLock::new(0)),
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
    total_supply: u64,
    total_txs: u64,
    current_leader: u64,
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
    pub signature: Option<String>,
    pub timestamp: Option<i64>,
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
    let ws = state.world_state.read().await;
    let height = blocks.last().map(|b| b.header.height).unwrap_or(0);
    let round = *state.current_round.read().await;
    Json(StatusResponse {
        node: "vela-node",
        version: "0.1.0",
        port: state.port,
        peer_count,
        block_height: height,
        mempool_size: mempool.len(),
        total_supply: ws.total_supply,
        total_txs: ws.total_txs,
        current_leader: round % 3,
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
            proposer: hex::encode(b.header.proposer.as_bytes()),
            round: b.header.round,
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: format!("Block {} not found", height) }),
        )),
    }
}

async fn get_balance(
    State(state): State<NodeState>,
    Path(address): Path<String>,
) -> Json<serde_json::Value> {
    let addr_hex = address.trim_start_matches("vela:");
    let ws = state.world_state.read().await;
    let mut addr_arr = [0u8; 32];
    if let Ok(bytes) = hex::decode(addr_hex) {
        if bytes.len() == 32 {
            addr_arr.copy_from_slice(&bytes);
        }
    }
    let addr = Address(addr_arr);
    let balance = ws.balance(&addr);
    let nonce = ws.nonce(&addr);
    Json(serde_json::json!({
        "address": address,
        "balance": balance,
        "nonce": nonce
    }))
}

async fn get_transactions(
    State(state): State<NodeState>,
    Path(address): Path<String>,
) -> Json<serde_json::Value> {
    let addr_hex = address.trim_start_matches("vela:").to_string();
    let blocks = state.blocks.read().await;
    let mut txs = vec![];
    for block in blocks.iter() {
        for tx in &block.transactions {
            let from_hex = hex::encode(tx.from.as_bytes());
            let to_hex = hex::encode(tx.to.as_bytes());
            if from_hex == addr_hex || to_hex == addr_hex {
                txs.push(serde_json::json!({
                    "hash": tx.hash().to_hex(),
                    "from": format!("vela:{}", from_hex),
                    "to": format!("vela:{}", to_hex),
                    "amount": tx.amount,
                    "fee": tx.fee,
                    "nonce": tx.nonce,
                    "block": block.header.height,
                    "timestamp": block.header.timestamp,
                }));
            }
        }
    }
    Json(serde_json::json!({ "address": address, "transactions": txs }))
}

async fn get_validators(
    State(state): State<NodeState>,
) -> Json<serde_json::Value> {
    use ed25519_dalek::SigningKey;
    let round = *state.current_round.read().await;
    let current_leader = round % 3;
    let validators: Vec<serde_json::Value> = (0..3).map(|i| {
        let mut seed = [0u8; 32];
        seed[0] = i as u8 + 1;
        let key = SigningKey::from_bytes(&seed);
        let addr = Address::from_pubkey(&key.verifying_key());
        serde_json::json!({
            "index": i,
            "address": format!("vela:{}", hex::encode(addr.as_bytes())),
            "is_leader": i == current_leader as usize,
            "stake": 1000
        })
    }).collect();
    Json(serde_json::json!({ "validators": validators, "current_leader": current_leader }))
}

async fn post_faucet(
    State(state): State<NodeState>,
    Path(address): Path<String>,
) -> Result<Json<TxResponse>, (StatusCode, Json<ErrorResponse>)> {
    use ed25519_dalek::SigningKey;
    {
        let ws = state.world_state.read().await;
        if !ws.can_faucet(&address) {
            return Err((StatusCode::TOO_MANY_REQUESTS, Json(ErrorResponse {
                error: "Faucet cooldown: 24 hours between requests".into()
            })));
        }
    }
    let to_bytes = hex::decode(address.trim_start_matches("vela:"))
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid address".into() })))?;
    if to_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "address must be 32 bytes".into() })));
    }
    let mut to_arr = [0u8; 32];
    to_arr.copy_from_slice(&to_bytes);
    let faucet_key = SigningKey::from_bytes(&FAUCET_SEED);
    let faucet_addr = Address::from_pubkey(&faucet_key.verifying_key());
    let nonce = {
        let ws = state.world_state.read().await;
        ws.nonce(&faucet_addr) + 1
    };
    let mut tx = Transaction::new(faucet_addr, Address(to_arr), FAUCET_AMOUNT, 1, nonce);
    tx.sign(&faucet_key);
    let tx_hash = tx.hash().to_hex();
    state.world_state.write().await.mark_faucet(&address);
    state.mempool.write().await.push(tx.clone());
    info!("💧 Faucet sent {} VELA to {}", FAUCET_AMOUNT, address);
    state.tx_broadcast.send(tx).await.ok();
    Ok(Json(TxResponse { status: "accepted".into(), tx_hash }))
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
    let from_addr = Address(from_arr);
    {
        let ws = state.world_state.read().await;
        let balance = ws.balance(&from_addr);
        let expected_nonce = ws.nonce(&from_addr) + 1;
        if balance < req.amount + req.fee {
            return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                error: format!("insufficient balance: have {}, need {}", balance, req.amount + req.fee)
            })));
        }
        if req.nonce != expected_nonce {
            return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                error: format!("invalid nonce: expected {}, got {}", expected_nonce, req.nonce)
            })));
        }
    }
    let mut tx = Transaction::new(from_addr, Address(to_arr), req.amount, req.fee, req.nonce);
    if let Some(ts) = req.timestamp {
        tx.timestamp = ts;
    }
    if let Some(sig_hex) = req.signature {
        if let Ok(sig_bytes) = hex::decode(&sig_hex) {
            tx.signature = Some(sig_bytes);
        }
    }
    if tx.signature.is_some() && !tx.verify() {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid signature".into() })));
    }
    let tx_hash = tx.hash().to_hex();
    {
        let mempool = state.mempool.read().await;
        if mempool.iter().any(|t| t.hash().to_hex() == tx_hash) {
            return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "tx already in mempool".into() })));
        }
    }
    state.mempool.write().await.push(tx.clone());
    info!("💸 TX added to mempool: {}", &tx_hash[..16]);
    state.tx_broadcast.send(tx).await.ok();
    Ok(Json(TxResponse { status: "accepted".into(), tx_hash }))
}

async fn get_tx(
    State(state): State<NodeState>,
    Path(hash): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let blocks = state.blocks.read().await;
    for block in blocks.iter() {
        for tx in &block.transactions {
            if tx.hash().to_hex() == hash {
                let from_hex = hex::encode(tx.from.as_bytes());
                let to_hex = hex::encode(tx.to.as_bytes());
                return Ok(Json(serde_json::json!({
                    "hash": tx.hash().to_hex(),
                    "from": format!("vela:{}", from_hex),
                    "to": format!("vela:{}", to_hex),
                    "amount": tx.amount,
                    "fee": tx.fee,
                    "nonce": tx.nonce,
                    "block": block.header.height,
                    "timestamp": block.header.timestamp,
                })));
            }
        }
    }
    Err((StatusCode::NOT_FOUND, Json(ErrorResponse { error: "transaction not found".into() })))
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
        .route("/transactions/{address}", get(get_transactions))
        .route("/validators", get(get_validators))
        .route("/faucet/{address}", post(post_faucet))
        .route("/send_tx", post(post_send_tx))
        .route("/tx/{hash}", get(get_tx))
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