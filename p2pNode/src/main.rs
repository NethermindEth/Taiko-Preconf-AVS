use crate::discovery::Discovery;
use crate::peer_manager::PeerManager;
use libp2p::futures::StreamExt;
use libp2p::gossipsub::{MessageAuthenticity, ValidationMode};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::SwarmBuilder;
use libp2p::{core::upgrade, gossipsub, identify, identity, noise, PeerId};
use libp2p_mplex::{MaxBufferBehaviour, MplexConfig};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tracing::{debug, info, warn};
mod discovery;
mod enr;
mod peer_manager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup tracing
    let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new("debug"))
        .unwrap();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter_layer)
        .try_init();
    // Create a random PeerId
    let local_key = identity::Keypair::generate_secp256k1();
    let local_peer_id = PeerId::from(local_key.public());

    info!("Local peer id: {local_peer_id}");

    let discovery = Discovery::new(&local_key).await;

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
    let mut gossipsub = gossipsub::Behaviour::new(MessageAuthenticity::Anonymous, gossipsub_config)
        .expect("Correct configuration");

    // Create a Gossipsub topic
    let topic = gossipsub::IdentTopic::new("taiko-avs");

    // subscribes to our topic
    gossipsub.subscribe(&topic)?;

    // Set a custom identify configuration
    let identify = identify::Behaviour::new(
        identify::Config::new("".into(), local_key.public()).with_cache_size(0),
    );

    // We create a custom network behaviour that combines Gossipsub and Discv5.
    #[derive(NetworkBehaviour)]
    struct Behaviour {
        gossipsub: gossipsub::Behaviour,
        discovery: Discovery,
        identify: identify::Behaviour,
        peer_manager: PeerManager,
    }

    let behaviour = {
        Behaviour {
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

    // yamux config
    let yamux_config = libp2p::yamux::Config::default();

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default().nodelay(true),
            noise::Config::new,
            || upgrade::SelectUpgrade::new(yamux_config, mplex_config),
        )
        .expect("building p2p transport failed")
        .with_behaviour(|_| behaviour)
        .expect("building p2p behaviour failed")
        .build();

    // Listen
    swarm.listen_on("/ip4/0.0.0.0/tcp/9000".parse()?)?;

    // Load SEND from env
    let send_prefix = std::env::var("SEND_PREFIX").unwrap();
    info!("SEND PREFIX: {send_prefix}");
    let mut send_count = 1;
    let mut send_interval = tokio::time::interval(Duration::from_secs(20));
    // Run
    loop {
        tokio::select! {
            _ = send_interval.tick() => {
                send_count += 1;
                let data = format!("{send_prefix}-{send_count}");
                debug!("SEND EVENT: {:#?}", &data);
                if let Err(e) = swarm
                    .behaviour_mut().gossipsub
                    .publish(topic.clone(), data.as_bytes()) {
                    warn!("Publish error: {e:?}");
                }
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(behaviour_event) => match behaviour_event {
                    BehaviourEvent::Gossipsub(gs) =>
                    if let gossipsub::Event::Message {
                        propagation_source: peer_id,
                        message_id: id,
                        message, } = gs { debug!("Got message: '{}' with id: {id} from peer: {peer_id}", String::from_utf8_lossy(&message.data)) },
                    BehaviourEvent::Discovery(discovered) => {
                        debug!("Discovery Event: {:#?}", &discovered);
                        swarm.behaviour_mut().peer_manager.add_peers(discovered.peers);
                    },
                    BehaviourEvent::Identify(ev) => {
                        debug!("identify: {:#?}", ev);
                        if let libp2p::identify::Event::Received { peer_id, info, .. } = ev {
                            swarm.behaviour_mut().peer_manager.add_peer_identity(peer_id, info);
                        }
                    },
                    BehaviourEvent::PeerManager(ev) => {
                        debug!("PeerManager event: {:#?}", ev);
                        match ev {
                            peer_manager::PeerManagerEvent::DiscoverPeers(num_peers) => {
                                swarm.behaviour_mut().discovery.set_peers_to_discover(num_peers as usize);
                            },
                            peer_manager::PeerManagerEvent::DialPeers(peer_ids) => {
                                debug!("DialPeers: {peer_ids:?}");
                                for peer_id in peer_ids {
                                    let addr = swarm.behaviour_mut().peer_manager.addresses_of_peer(&peer_id);
                                    debug!("Peer: {peer_id:?} - Addr: {addr:?}");
                                    if !addr.is_empty() {
                                        let _ = swarm.dial(addr[0].clone());
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
                info!("Connected peers: {}", swarm.behaviour_mut().gossipsub.all_peers().collect::<Vec<_>>().len());
            }
        }
    }
}
