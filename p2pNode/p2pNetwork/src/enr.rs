use crate::network::P2PNetworkConfig;
use discv5::{
    enr::{self, CombinedKey, CombinedPublicKey},
    Enr,
};
use libp2p::identity::{ed25519, secp256k1, PublicKey};
use libp2p::PeerId;

pub fn build_enr(config: &P2PNetworkConfig, combined_key: &CombinedKey) -> Enr {
    let mut enr_builder = enr::Enr::builder();
    enr_builder.ip4(config.ipv4);
    enr_builder.udp4(config.udpv4);
    enr_builder.tcp4(config.tcpv4);
    enr_builder.build(combined_key).unwrap()
}

pub trait EnrAsPeerId {
    /// Converts the enr into a peer id
    fn as_peer_id(&self) -> PeerId;
}

impl EnrAsPeerId for Enr {
    fn as_peer_id(&self) -> PeerId {
        let public_key = self.public_key();

        match public_key {
            CombinedPublicKey::Secp256k1(pk) => {
                let pk_bytes = pk.to_sec1_bytes();
                let libp2p_pk: PublicKey = secp256k1::PublicKey::try_from_bytes(&pk_bytes)
                    .expect("valid public key")
                    .into();
                PeerId::from_public_key(&libp2p_pk)
            }
            CombinedPublicKey::Ed25519(pk) => {
                let pk_bytes = pk.to_bytes();
                let libp2p_pk: PublicKey = ed25519::PublicKey::try_from_bytes(&pk_bytes)
                    .expect("valid public key")
                    .into();
                PeerId::from_public_key(&libp2p_pk)
            }
        }
    }
}
