use crate::discovery::Discovery;
use crate::peer_manager::PeerManager;
use libp2p::futures::StreamExt;
use libp2p::gossipsub::{MessageAuthenticity, ValidationMode};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{gossipsub, identify, identity, noise, PeerId};
use libp2p::{Multiaddr, SwarmBuilder};
use libp2p_mplex::{MaxBufferBehaviour, MplexConfig};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, info, warn};

pub struct P2PNetworkConfig {
    pub local_key: identity::Keypair,
    pub listen_addr: Multiaddr,
    pub ipv4: Ipv4Addr,
    pub udpv4: u16,
    pub tcpv4: u16,
    pub boot_nodes: Option<Vec<String>>,
}
#[derive(NetworkBehaviour)]
struct SwarmBehaviour {
    gossipsub: gossipsub::Behaviour,
    discovery: Discovery,
    identify: identify::Behaviour,
    peer_manager: PeerManager,
}

pub struct P2PNetwork {
    node_tx: Sender<Vec<u8>>,
    node_to_p2p_rx: Receiver<Vec<u8>>,
    swarm: libp2p::Swarm<SwarmBehaviour>,
    topic_name: String,
}

impl fmt::Display for P2PNetworkConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "P2PNetworkConfig {{\n  listen_addr: {},\n  ipv4: {},\n  udpv4: {},\n  tcpv4: {},\n  boot_nodes: {:?}\n}}",
            self.listen_addr,
            self.ipv4,
            self.udpv4,
            self.tcpv4,
            self.boot_nodes
        )
    }
}

impl P2PNetwork {
    pub async fn new(
        config: &P2PNetworkConfig,
        node_tx: Sender<Vec<u8>>,
        node_to_p2p_rx: Receiver<Vec<u8>>,
    ) -> Self {
        // Create a random PeerId
        let local_peer_id = PeerId::from(config.local_key.public());

        info!("Local peer id: {local_peer_id}");

        let discovery = Discovery::new(config, &config.local_key).await;

        let target_num_peers = 16;
        let peer_manager = PeerManager::new(target_num_peers);
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        // Set a custom gossipsub configuration
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .max_transmit_size(10 * 1_048_576)
            .fanout_ttl(Duration::from_secs(60))
            .heartbeat_interval(Duration::from_millis(10_000))
            .validation_mode(ValidationMode::Anonymous)
            .fanout_ttl(Duration::from_secs(60))
            .history_length(12)
            .max_messages_per_rpc(Some(500))
            .message_id_fn(message_id_fn)
            .build()
            .expect("Valid config");

        // build a gossipsub network behaviour
        let mut gossipsub =
            gossipsub::Behaviour::new(MessageAuthenticity::Anonymous, gossipsub_config)
                .expect("Correct configuration");

        // Create a Gossipsub topic
        let topic_name = "taiko-avs".to_string();
        let topic = gossipsub::IdentTopic::new(topic_name.clone());

        // subscribes to our topic
        gossipsub.subscribe(&topic).unwrap();

        // Set a custom identify configuration
        let identify = identify::Behaviour::new(
            identify::Config::new("".into(), config.local_key.public()).with_cache_size(0),
        );

        // We create a custom network behaviour that combines Gossipsub and Discv5.

        let behaviour = {
            SwarmBehaviour {
                gossipsub,
                discovery,
                identify,
                peer_manager,
            }
        };

        // mplex config
        let mut mplex_config = MplexConfig::new();
        mplex_config.set_max_buffer_size(256);
        mplex_config.set_max_buffer_behaviour(MaxBufferBehaviour::Block);

        let swarm = SwarmBuilder::with_existing_identity(config.local_key.clone())
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default().nodelay(true),
                noise::Config::new,
                libp2p::yamux::Config::default,
            )
            .expect("building p2p transport failed")
            .with_behaviour(|_| behaviour)
            .expect("building p2p behaviour failed")
            .build();

        P2PNetwork {
            node_tx,
            node_to_p2p_rx,
            swarm,
            topic_name,
        }
    }

    pub fn get_local_enr(&self) -> String {
        self.swarm.behaviour().discovery.get_local_enr()
    }

    pub async fn run(&mut self, config: &P2PNetworkConfig) {
        info!("Starting P2P network");
        self.swarm.listen_on(config.listen_addr.clone()).unwrap();
        //loop
        loop {
            tokio::select! {
                Some(message) = self.node_to_p2p_rx.recv() => {
                    debug!("Sent message to p2p with size: {}", message.len());
                    let topic = gossipsub::IdentTopic::new(self.topic_name.clone());
                    //encode message
                    if let Err(e) = self.swarm
                        .behaviour_mut().gossipsub
                        .publish(topic, message) {
                        warn!("Publish error: {e:?}");
                    }
                }
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(behaviour_event) => match behaviour_event {
                        SwarmBehaviourEvent::Gossipsub(gs) =>
                        if let gossipsub::Event::Message {
                            propagation_source: peer_id,
                            message_id: id,
                            message, } = gs {
                                debug!("Got message: with id: {id} from peer: {peer_id}");
                                // decode message
                                if let Err(e) = self.node_tx
                                    .send(message.data)
                                    .await {
                                        warn!("Can't send message to node from network: {e:?}");
                                    }
                        },
                        SwarmBehaviourEvent::Discovery(discovered) => {
                            debug!("Discovery Event: {:#?}", &discovered);
                            self.swarm.behaviour_mut().peer_manager.add_peers(discovered.peers);
                        },
                        SwarmBehaviourEvent::Identify(ev) => {
                            debug!("identify: {:#?}", ev);
                            if let libp2p::identify::Event::Received { peer_id, info, .. } = ev {
                                self.swarm.behaviour_mut().peer_manager.add_peer_identity(peer_id, info);
                            }
                        },
                        SwarmBehaviourEvent::PeerManager(ev) => {
                            debug!("PeerManager event: {:#?}", ev);
                            match ev {
                                super::peer_manager::PeerManagerEvent::DiscoverPeers(num_peers) => {
                                    self.swarm.behaviour_mut().discovery.set_peers_to_discover(num_peers as usize);
                                },
                                super::peer_manager::PeerManagerEvent::DialPeers(peer_ids) => {
                                    debug!("DialPeers: {peer_ids:?}");
                                    for peer_id in peer_ids {
                                        let addr = self.swarm.behaviour_mut().peer_manager.addresses_of_peer(&peer_id);
                                        debug!("Peer: {peer_id:?} - Addr: {addr:?}");
                                        if !addr.is_empty() {
                                            let _ = self.swarm.dial(addr[0].clone());
                                        }
                                    }
                                },
                            }
                        },
                    },
                    SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, cause, .. } => {
                        debug!("ConnectionClosed: Cause {cause:?} - PeerId: {peer_id:?} - NumEstablished: {num_established:?} - Endpoint: {endpoint:?}");
                    },
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, .. } => {
                        debug!("ConnectionEstablished: PeerId: {peer_id:?} - NumEstablished: {num_established:?} - Endpoint: {endpoint:?}");
                    },
                    SwarmEvent::NewListenAddr { address, .. } => {
                        debug!("Local node is listening on {address}");
                    }
                    _ => debug!("Swarm: {event:?}"),
                },
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    debug!("Connected peers: {}", self.swarm.behaviour_mut().gossipsub.all_peers().collect::<Vec<_>>().len());
                }
            }
        }
    }
}
