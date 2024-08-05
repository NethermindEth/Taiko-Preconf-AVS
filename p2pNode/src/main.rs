use crate::discovery::Discovery;
use crate::peer_manager::PeerManager;
use libp2p::futures::StreamExt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use libp2p::gossipsub::{
     Gossipsub,  MessageAuthenticity, 
     ValidationMode,
};
use libp2p::swarm::{ConnectionLimits, NetworkBehaviour, SwarmBuilder, SwarmEvent};
use libp2p::{
    core, dns, gossipsub, identify, identity, mplex, noise, tcp, websocket, yamux, PeerId,
    Transport,
};
use tracing::{info, warn, debug};
use std::time::Duration;
mod discovery;
mod enr;
mod peer_manager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))
        .unwrap();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter_layer)
        .try_init();
    // Create a random PeerId
    let local_key = identity::Keypair::generate_secp256k1();
    let local_peer_id = PeerId::from(local_key.public());

    info!("Local peer id: {local_peer_id}");

    // Set up an encrypted DNS-enabled TCP Transport over the Mplex protocol.
    let transport = build_transport(&local_key)?;

    // Create discovery
    let discovery = Discovery::new(&local_key).await;

    let target_num_peers = 16;
    let peer_manager = PeerManager::new(target_num_peers);
    let message_id_fn = |message: &gossipsub::GossipsubMessage| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        gossipsub::MessageId::from(s.finish().to_string())
    };

    // Set a custom gossipsub configuration
    let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
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
    let mut gossipsub = Gossipsub::new(MessageAuthenticity::Anonymous, gossipsub_config)
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
        gossipsub: Gossipsub,
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

    // Create a Swarm to manage peers and events
    let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id)
        .notify_handler_buffer_size(std::num::NonZeroUsize::new(7).expect("Not zero"))
        .connection_event_buffer_size(64)
        .connection_limits(
            ConnectionLimits::default()
        )
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
                info!("Sending message: {:#?}", &data);
                if let Err(e) = swarm
                    .behaviour_mut().gossipsub
                    .publish(topic.clone(), data.as_bytes()) {
                    warn!("Publish error: {e:?}");
                }
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(behaviour_event) => match behaviour_event {
                    BehaviourEvent::Gossipsub(gs) =>
                    match gs {
                        gossipsub::GossipsubEvent::Message { 
                            propagation_source: peer_id,
                            message_id: id,
                            message, } => info!("Got message: '{}' with id: {id} from peer: {peer_id}", String::from_utf8_lossy(&message.data)),
                        _ => ()
                    },
                    BehaviourEvent::Discovery(discovered) => {
                        debug!("Discovery Event: {:#?}", &discovered);
                            swarm.behaviour_mut().peer_manager.add_peers(discovered.peers);
                    },
                    BehaviourEvent::Identify(ev) => {
                        debug!("identify: {:#?}", ev);
                        match ev {
                            libp2p::identify::Event::Received { peer_id, info, .. } => {
                                swarm.behaviour_mut().peer_manager.add_peer_identity(peer_id, info);
                            }
                            _ => {}
                        }
                    },
                    BehaviourEvent::PeerManager(ev) => {
                        debug!("PeerManager event: {:#?}", ev);
                        match ev {
                            peer_manager::PeerManagerEvent::DiscoverPeers(num_peers) => {
                                swarm.behaviour_mut().discovery.set_peers_to_discover(num_peers as usize);
                            },
                            peer_manager::PeerManagerEvent::DialPeers(peer_ids) => {
                                for peer_id in peer_ids {
                                    swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                }
                            },
                        }
                    },
                },
                SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, cause } => {
                    debug!("ConnectionClosed: Cause {cause:?} - PeerId: {peer_id:?} - NumEstablished: {num_established:?} - Endpoint: {endpoint:?}");
                },
                SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, .. } => {
                    debug!("ConnectionEstablished: PeerId: {peer_id:?} - NumEstablished: {num_established:?} - Endpoint: {endpoint:?}");
                },
                _ => debug!("Swarm: {event:?}"),
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                info!("Connected peers: {}", swarm.behaviour_mut().gossipsub.all_peers().collect::<Vec<_>>().len());
            }
        }
    }
}

pub fn build_transport(
    keypair: &identity::Keypair,
) -> std::io::Result<core::transport::Boxed<(PeerId, core::muxing::StreamMuxerBox)>> {
    let transport = {
        let dns_tcp = dns::TokioDnsConfig::system(tcp::tokio::Transport::new(
            tcp::Config::new().nodelay(true),
        ))?;
        let ws_dns_tcp = websocket::WsConfig::new(dns::TokioDnsConfig::system(
            tcp::tokio::Transport::new(tcp::Config::new().nodelay(true)),
        )?);
        dns_tcp.or_transport(ws_dns_tcp)
    };

    let mut mplex_config = mplex::MplexConfig::new();
    mplex_config.set_max_buffer_size(256);
    mplex_config.set_max_buffer_behaviour(mplex::MaxBufferBehaviour::Block);

    let mut yamux_config = yamux::YamuxConfig::default();
    yamux_config.set_window_update_mode(yamux::WindowUpdateMode::on_read());

    Ok(transport
        .upgrade(core::upgrade::Version::V1)
        .authenticate(generate_noise_config(keypair))
        .multiplex(core::upgrade::SelectUpgrade::new(
            yamux_config,
            mplex_config,
        ))
        .timeout(std::time::Duration::from_secs(100))
        .boxed())
}

fn generate_noise_config(
    identity_keypair: &identity::Keypair,
) -> noise::NoiseAuthenticated<noise::XX, noise::X25519Spec, ()> {
    let static_dh_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(identity_keypair)
        .expect("signing can fail only once during starting a node");
    noise::NoiseConfig::xx(static_dh_keys).into_authenticated()
}