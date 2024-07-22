use crate::enr::{build_enr, EnrAsPeerId};
use discv5::enr::NodeId;
use discv5::{enr::CombinedKey, Discv5, ConfigBuilder, Event, Enr, ListenConfig};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p::futures::FutureExt;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p::{Multiaddr, PeerId};
use log::{debug, warn};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc;
use std::net::Ipv4Addr;

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
    pub async fn new(local_key: &Keypair) -> Self {
        // Generate ENR
        let enr_key: CombinedKey = key_from_libp2p(local_key).unwrap();

        let local_enr = build_enr(&enr_key);

        // listening address and port
        let listen_config = ListenConfig::Ipv4 {
            ip: Ipv4Addr::UNSPECIFIED,
            port: 9000,
        };

        // Setup default config
        let config = ConfigBuilder::new(listen_config)
            .enable_packet_filter()
            .request_timeout(Duration::from_secs(1))
            .query_peer_timeout(Duration::from_secs(2))
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
        let mut discv5 = Discv5::new(local_enr.clone(), enr_key, config).unwrap();

        // Add bootnode
        /*for bootnode in BOOTNODES {
            let ef_bootnode_enr = Enr::from_str(bootnode).unwrap();
            discv5.add_enr(ef_bootnode_enr).expect("bootnode error");
        }*/

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

                println!("Found {} peers", peers.len());
                println!("Peers: {:#?}", &peers);
                return Some(DiscoveredPeers { peers });
            }
        }

        None
    }
}

enum EventStream {
    Present(mpsc::Receiver<Event>),
    InActive,
    Awaiting(
        Pin<
            Box<
                dyn Future<Output = Result<mpsc::Receiver<Event>, discv5::Error>>
                    + Send,
            >,
        >,
    ),
}

impl NetworkBehaviour for Discovery {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type OutEvent = DiscoveredPeers;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        libp2p::swarm::dummy::ConnectionHandler {}
    }

    // Main execution loop to drive the behaviour
    fn poll(
        &mut self,
        cx: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        // println!("Discovery polled : {}", self.poll_count);
        if self.peers_to_discover > 0 {
            self.started = true;
            println!("Finding Peers");
            self.find_peers(self.peers_to_discover);
            self.peers_to_discover = 0;

            return Poll::Pending;
        }

        if let Some(dp) = self.get_peers(cx) {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(dp));
        };

        // Process the discovery server event stream
        match self.event_stream {
            EventStream::Awaiting(ref mut fut) => {
                // Still awaiting the event stream, poll it
                if let Poll::Ready(event_stream) = fut.poll_unpin(cx) {
                    match event_stream {
                        Ok(stream) => {
                            println!("Discv5 event stream ready");
                            self.event_stream = EventStream::Present(stream);
                        }
                        Err(_) => {
                            println!("Discv5 event stream failed");
                            self.event_stream = EventStream::InActive;
                        }
                    }
                }
            }
            EventStream::InActive => {}
            EventStream::Present(ref mut stream) => {
                while let Poll::Ready(Some(event)) = stream.poll_recv(cx) {
                    match event {
                        Event::SessionEstablished(_enr, _) => {
                            // println!("Session Established: {:?}", enr);
                        }
                        _ => (),
                    }
                }
            }
        }
        Poll::Pending
    }
}

// Get CombinedKey from Secp256k1 libp2p Keypair
pub fn key_from_libp2p(key: &libp2p::core::identity::Keypair) -> Result<CombinedKey, &'static str> {
    match key {
        Keypair::Secp256k1(key) => {
            let secret = discv5::enr::k256::ecdsa::SigningKey::from_bytes(&key.secret().to_bytes().into())
                .expect("libp2p key must be valid");
            Ok(CombinedKey::Secp256k1(secret))
        }
        _ => Err("pair not supported"),
    }
}