use crate::enr::{build_enr, EnrAsPeerId};
use discv5::enr::NodeId;
use discv5::{enr::CombinedKey, Discv5, Event, Enr, ConfigBuilder, ListenConfig};
use std::net::Ipv4Addr;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p::futures::FutureExt;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{NetworkBehaviourAction, PollParameters};
use libp2p::Multiaddr;
use libp2p::{swarm::NetworkBehaviour, PeerId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::mpsc;

pub struct Discovery {
    discv5: Discv5,
    _enr: Enr,
    event_stream: EventStream,
    multiaddr_map: HashMap<PeerId, Multiaddr>,
    peers_future: FuturesUnordered<std::pin::Pin<Box<dyn Future<Output = DiscResult> + Send>>>,
    started: bool,
}

type DiscResult = Result<Vec<discv5::enr::Enr<CombinedKey>>, discv5::QueryError>;

#[derive(Debug, Clone)]
pub struct DiscoveredPeers {
    pub peers: HashMap<PeerId, Option<Instant>>,
}

impl Discovery {
    pub async fn new(local_key: &Keypair) -> Self {
        // listening address and port
        let listen_config = ListenConfig::Ipv4 {
            ip: Ipv4Addr::UNSPECIFIED,
            port: 9000,
        };
    
        // convert the keypair into an ENR key
        let enr_key: CombinedKey = key_from_libp2p(local_key).unwrap();
        // construct a local ENR
        let enr = build_enr(&enr_key);
    
        // if the ENR is useful print it
        println!("Node Id: {}", enr.node_id());
        if enr.udp4_socket().is_some() {
            println!("Base64 ENR: {}", enr.to_base64());
            println!(
                "IP: {}, UDP_PORT:{}",
                enr.ip4().unwrap(),
                enr.udp4().unwrap()
            );
        } else {
            println!("ENR is not printed as no IP:PORT was specified");
        }
    
        // default configuration
        let config = ConfigBuilder::new(listen_config).build();
        
        // Create discv5 instance
        let mut discv5 = Discv5::new(enr.clone(), enr_key, config).unwrap();

        // Start the discv5 service
        discv5.start().await.unwrap();

        // Obtain an event stream
        let event_stream = EventStream::Awaiting(Box::pin(discv5.event_stream()));

        return Self {
            discv5,
            _enr: enr,
            event_stream,
            multiaddr_map: HashMap::new(),
            peers_future: FuturesUnordered::new(),
            started: false,
        };
    }

    fn find_peers(&mut self) {
        let predicate: Box<dyn Fn(&Enr) -> bool + Send> =
            Box::new(move |enr: &Enr| enr.tcp4().is_some() && enr.udp4().is_some());

        let target = NodeId::random();

        let peers_enr = self.discv5.find_node_predicate(target, predicate, 16);

        self.peers_future.push(Box::pin(peers_enr));
    }

    fn get_peers(&mut self, cx: &mut Context) -> Option<DiscoveredPeers> {
        while let Poll::Ready(Some(res)) = self.peers_future.poll_next_unpin(cx) {
            if res.is_ok() {
                self.peers_future = FuturesUnordered::new();

                let mut peers: HashMap<PeerId, Option<Instant>> = HashMap::new();

                for peer_enr in res.unwrap() {
                    let peer_id = peer_enr.clone().as_peer_id();

                    if peer_enr.ip4().is_some() && peer_enr.tcp4().is_some() {
                        let mut multiaddr: Multiaddr = peer_enr.ip4().unwrap().into();

                        multiaddr.push(Protocol::Tcp(peer_enr.tcp4().unwrap()));

                        self.multiaddr_map.insert(peer_id, multiaddr);
                    }

                    peers.insert(peer_id, None);
                }

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

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        let mut peer_address: Vec<Multiaddr> = Vec::new();

        if let Some(address) = self.multiaddr_map.get(peer_id) {
            peer_address.push(address.clone());
        }

        return peer_address;
    }

    // Main execution loop to drive the behaviour
    fn poll(
        &mut self,
        cx: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if !self.started {
            self.started = true;
            self.find_peers();

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
                        Event::SessionEstablished(enr, _) => {
                            println!("Session Established: {:?}", enr);
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