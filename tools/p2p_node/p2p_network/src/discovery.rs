use crate::enr::{build_enr, EnrAsPeerId};
use crate::network::P2PNetworkConfig;
use discv5::enr::NodeId;
use discv5::{enr::CombinedKey, ConfigBuilder, Discv5, Enr, Event, ListenConfig};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p::futures::FutureExt;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::behaviour::{DialFailure, FromSwarm};
use libp2p::swarm::THandlerInEvent;
use libp2p::swarm::{dummy::ConnectionHandler, ConnectionId};
use libp2p::swarm::{NetworkBehaviour, ToSwarm};
use libp2p::{Multiaddr, PeerId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

pub struct Discovery {
    discv5: Discv5,
    _enr: Enr,
    event_stream: EventStream,
    peers_future: FuturesUnordered<std::pin::Pin<Box<dyn Future<Output = DiscResult> + Send>>>,
    peers_to_discover: usize,
    started: bool,
}

type DiscResult = Result<Vec<discv5::enr::Enr<CombinedKey>>, discv5::QueryError>;

#[derive(Debug, Clone)]
pub struct DiscoveredPeers {
    pub peers: HashMap<PeerId, Option<Multiaddr>>,
}

impl Discovery {
    pub async fn new(config: &P2PNetworkConfig, local_key: &Keypair) -> Self {
        // Generate ENR
        let enr_key: CombinedKey = key_from_libp2p(local_key.clone()).unwrap();

        let local_enr = build_enr(config, &enr_key);

        info!("Node Id: {:?}", local_enr.node_id());
        if local_enr.udp4_socket().is_some() {
            info!("Base64 ENR: {}", local_enr.to_base64());
            info!(
                "IP: {}, UDP_PORT:{}",
                local_enr.ip4().unwrap(),
                local_enr.udp4().unwrap()
            );
        } else {
            warn!("ENR is not printed as no IP:PORT was specified");
        }

        // listening address and port
        let listen_config = ListenConfig::Ipv4 {
            ip: local_enr.ip4().unwrap(),
            port: 9000,
        };

        // Setup default config
        let discv5_config = ConfigBuilder::new(listen_config)
            .enable_packet_filter()
            .request_timeout(Duration::from_secs(30))
            .query_peer_timeout(Duration::from_secs(30))
            .query_timeout(Duration::from_secs(30))
            .request_retries(1)
            .enr_peer_update_min(10)
            .query_parallelism(5)
            .disable_report_discovered_peers()
            .ip_limit()
            .incoming_bucket_limit(8)
            .ping_interval(Duration::from_secs(300))
            .build();

        // Create discv5 instance
        let mut discv5 = Discv5::new(local_enr.clone(), enr_key, discv5_config).unwrap();

        // Process bootnode
        if let Some(vec) = config.boot_nodes.clone() {
            for enr_str in vec {
                if let Ok(bootnode_enr) = Enr::from_str(&enr_str) {
                    info!("Add boot node ENR: {}", enr_str);
                    discv5.add_enr(bootnode_enr).expect("Add bootnode error");
                } else {
                    warn!("Failed to parse bootnode ENR: {}", enr_str);
                }
            }
        } else {
            info!("No bootnodes specified");
        }

        // Start the discv5 service
        discv5.start().await.unwrap();

        // Obtain an event stream
        let event_stream = EventStream::Awaiting(Box::pin(discv5.event_stream()));

        Self {
            discv5,
            _enr: local_enr,
            event_stream,
            peers_future: FuturesUnordered::new(),
            started: false,
            peers_to_discover: 0,
        }
    }

    pub fn set_peers_to_discover(&mut self, peers_to_discover: usize) {
        self.peers_to_discover = peers_to_discover;
    }

    fn find_peers(&mut self, count: usize) {
        let predicate = Box::new(|enr: &Enr| enr.ip4().is_some());

        let target = NodeId::random();

        let peers_enr = self.discv5.find_node_predicate(target, predicate, count);

        self.peers_future.push(Box::pin(peers_enr));
    }

    pub fn get_local_enr(&self) -> String {
        self.discv5.local_enr().to_base64()
    }

    fn get_peers(&mut self, cx: &mut Context) -> Option<DiscoveredPeers> {
        while let Poll::Ready(Some(res)) = self.peers_future.poll_next_unpin(cx) {
            if res.is_ok() {
                self.peers_future = FuturesUnordered::new();

                let mut peers: HashMap<PeerId, Option<Multiaddr>> = HashMap::new();

                for peer_enr in res.unwrap() {
                    match self.discv5.add_enr(peer_enr.clone()) {
                        Ok(_) => {
                            debug!("Added peer: {:?} to discv5", peer_enr.node_id());
                        }
                        Err(_) => {
                            warn!("Failed to add peer: {:?} to discv5", peer_enr.node_id());
                        }
                    };
                    let peer_id = peer_enr.clone().as_peer_id();

                    let mut multiaddr: Option<Multiaddr> = None;
                    if peer_enr.ip4().is_some() && peer_enr.tcp4().is_some() {
                        let mut multiaddr_inner: Multiaddr = peer_enr.ip4().unwrap().into();
                        multiaddr_inner.push(Protocol::Tcp(peer_enr.tcp4().unwrap()));
                        multiaddr = Some(multiaddr_inner);
                    }
                    peers.insert(peer_id, multiaddr);
                }

                debug!("Found peers: {:#?}", &peers);
                return Some(DiscoveredPeers { peers });
            }
        }

        None
    }
}

enum EventStream {
    Present(mpsc::Receiver<Event>),
    InActive,
    Awaiting(Pin<Box<dyn Future<Output = Result<mpsc::Receiver<Event>, discv5::Error>> + Send>>),
}

impl NetworkBehaviour for Discovery {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = DiscoveredPeers;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        // TODO: we might want to check discovery's banned ips here in the future.
        Ok(ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
        _port_use: libp2p::core::transport::PortUse,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(ConnectionHandler)
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        _event: void::Void,
    ) {
    }

    // Main execution loop to drive the behaviour
    fn poll(&mut self, cx: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if self.peers_to_discover > 0 {
            self.started = true;
            debug!("Finding Peers");
            self.find_peers(self.peers_to_discover);
            self.peers_to_discover = 0;

            return Poll::Pending;
        }

        if let Some(dp) = self.get_peers(cx) {
            return Poll::Ready(ToSwarm::GenerateEvent(dp));
        };

        // Process the discovery server event stream
        match self.event_stream {
            EventStream::Awaiting(ref mut fut) => {
                // Still awaiting the event stream, poll it
                if let Poll::Ready(event_stream) = fut.poll_unpin(cx) {
                    match event_stream {
                        Ok(stream) => {
                            debug!("Discv5 event stream ready");
                            self.event_stream = EventStream::Present(stream);
                        }
                        Err(_) => {
                            warn!("Discv5 event stream failed");
                            self.event_stream = EventStream::InActive;
                        }
                    }
                }
            }
            EventStream::InActive => {}
            EventStream::Present(ref mut stream) => {
                while let Poll::Ready(Some(event)) = stream.poll_recv(cx) {
                    if let Event::SessionEstablished(_enr, _) = event {
                        debug!("Session Established: {:?}", _enr);
                    }
                }
            }
        }
        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        debug!("on_swarm_event event: {:?}", event);
        match event {
            FromSwarm::DialFailure(DialFailure { peer_id, .. }) => {
                debug!("DialFailure event from swarm {:?}", peer_id);
            }
            FromSwarm::NewListenAddr(ev) => {
                debug!("Received NewListenAddr event from swarm ev = {ev:?}");
            }
            _ => {
                // Ignore events not relevant to discovery
            }
        }
    }
}

// Get CombinedKey from Secp256k1 libp2p Keypair
pub fn key_from_libp2p(key: libp2p::identity::Keypair) -> Result<CombinedKey, &'static str> {
    match key.key_type() {
        libp2p::identity::KeyType::Secp256k1 => {
            let key = key.try_into_secp256k1().expect("right key type");
            let secret = discv5::enr::k256::ecdsa::SigningKey::from_slice(&key.secret().to_bytes())
                .expect("libp2p key must be valid");
            Ok(CombinedKey::Secp256k1(secret))
        }
        _ => Err("pair not supported"),
    }
}
