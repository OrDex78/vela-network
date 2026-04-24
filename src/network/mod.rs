use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use anyhow::Result;
use futures::StreamExt;
use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    mdns,
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::types::{Block, Transaction, Vote};

pub const TOPIC_BLOCKS: &str = "vela-blocks";
pub const TOPIC_TXS: &str = "vela-transactions";
pub const TOPIC_CONSENSUS: &str = "vela-consensus";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    NewBlock(Block),
    NewTransaction(Transaction),
    ConsensusVote(Vote),
    ConsensusPropose(Block),
    SyncRequest { from_height: u64 },
    SyncResponse { blocks: Vec<Block> },
}

#[derive(NetworkBehaviour)]
pub struct VelaBehaviour {
    gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

pub struct P2PNode {
    pub local_peer_id: PeerId,
    port: u16,
    bootstrap_peers: Vec<Multiaddr>,
    pub tx_out: mpsc::Sender<NetworkMessage>,
    rx_out: mpsc::Receiver<NetworkMessage>,
    pub tx_in: mpsc::Sender<NetworkMessage>,
}

impl P2PNode {
    pub fn new(
        port: u16,
        bootstrap_peers: Vec<Multiaddr>,
        tx_in: mpsc::Sender<NetworkMessage>,
    ) -> Result<Self> {
        let (tx_out, rx_out) = mpsc::channel(256);
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(keypair.public());
        info!("Local peer id: {local_peer_id}");
        Ok(Self {
            local_peer_id,
            port,
            bootstrap_peers,
            tx_out,
            rx_out,
            tx_in,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        let keypair = libp2p::identity::Keypair::generate_ed25519();

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(ValidationMode::Permissive)
            .message_id_fn(|msg: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                msg.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            })
            .build()
            .expect("valid gossipsub config");

        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_dns()?
            .with_behaviour(|key| {
                let gossipsub = gossipsub::Behaviour::new(
                    MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )
                .expect("valid gossipsub behaviour");

                let mdns = mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    key.public().to_peer_id(),
                )
                .expect("valid mdns behaviour");

                VelaBehaviour { gossipsub, mdns }
            })?
            .with_swarm_config(|c| {
                c.with_idle_connection_timeout(Duration::from_secs(60))
            })
            .build();

        let topic_blocks = IdentTopic::new(TOPIC_BLOCKS);
        let topic_txs = IdentTopic::new(TOPIC_TXS);
        let topic_consensus = IdentTopic::new(TOPIC_CONSENSUS);
        swarm.behaviour_mut().gossipsub.subscribe(&topic_blocks)?;
        swarm.behaviour_mut().gossipsub.subscribe(&topic_txs)?;
        swarm.behaviour_mut().gossipsub.subscribe(&topic_consensus)?;

        let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", self.port)
            .parse()
            .expect("valid multiaddr");
        swarm.listen_on(listen_addr)?;

        for addr in &self.bootstrap_peers {
            match swarm.dial(addr.clone()) {
                Ok(_) => info!("Dialing bootstrap peer: {addr}"),
                Err(e) => warn!("Failed to dial {addr}: {e}"),
            }
        }

        info!("Vela node listening on port {} | peer: {}", self.port, swarm.local_peer_id());

        let retry_peers = self.bootstrap_peers.clone();
        let mut retry_interval = tokio::time::interval(Duration::from_secs(30));
        retry_interval.tick().await; // consume immediate first tick

        loop {
            tokio::select! {
                Some(msg) = self.rx_out.recv() => {
                    let (topic, data) = match &msg {
                        NetworkMessage::NewBlock(_) => (&topic_blocks, serde_json::to_vec(&msg)?),
                        NetworkMessage::NewTransaction(_) => (&topic_txs, serde_json::to_vec(&msg)?),
                        NetworkMessage::ConsensusVote(_) => (&topic_consensus, serde_json::to_vec(&msg)?),
                        NetworkMessage::ConsensusPropose(_) => (&topic_consensus, serde_json::to_vec(&msg)?),
                        NetworkMessage::SyncRequest { .. } => (&topic_blocks, serde_json::to_vec(&msg)?),
                        NetworkMessage::SyncResponse { .. } => (&topic_blocks, serde_json::to_vec(&msg)?),
                    };
                    if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic.clone(), data) {
                        warn!("Gossipsub publish: {e}");
                    }
                }

                event = swarm.select_next_some() => {
                    match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            info!("Listening on {address}");
                        }
                        SwarmEvent::Behaviour(VelaBehaviourEvent::Mdns(
                            mdns::Event::Discovered(list),
                        )) => {
                            for (peer_id, addr) in list {
                                info!("mDNS discovered peer: {peer_id} @ {addr}");
                                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                            }
                        }
                        SwarmEvent::Behaviour(VelaBehaviourEvent::Mdns(
                            mdns::Event::Expired(list),
                        )) => {
                            for (peer_id, _) in list {
                                warn!("mDNS peer expired: {peer_id}");
                                swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                            }
                        }
                        SwarmEvent::Behaviour(VelaBehaviourEvent::Gossipsub(
                            gossipsub::Event::Message { message, .. },
                        )) => {
                            match serde_json::from_slice::<NetworkMessage>(&message.data) {
                                Ok(msg) => {
                                    if let Err(e) = self.tx_in.send(msg).await {
                                        error!("Failed to forward message: {e}");
                                    }
                                }
                                Err(e) => warn!("Failed to decode message: {e}"),
                            }
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            info!("✅ Connected to peer: {}", peer_id);
                        }
                        SwarmEvent::OutgoingConnectionError { error, .. } => {
                            warn!("❌ Failed to connect to bootstrap: {}", error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                            warn!("🔌 Connection closed: {} — {:?}", peer_id, cause);
                        }
                        _ => {}
                    }
                }
                _ = retry_interval.tick() => {
                    for addr in &retry_peers {
                        match swarm.dial(addr.clone()) {
                            Ok(_) => info!("Retrying bootstrap peer: {addr}"),
                            Err(e) => warn!("Failed to retry {addr}: {e}"),
                        }
                    }
                }
            }
        }
    }
}