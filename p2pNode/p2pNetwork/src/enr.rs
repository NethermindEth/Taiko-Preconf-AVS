use crate::network::P2PNetworkConfig;
use anyhow::{anyhow, Result};
use discv5::{
    enr::{self, CombinedKey, CombinedPublicKey},
    Enr,
};
use libp2p::identity::{ed25519, secp256k1};
use libp2p::PeerId;

/// Builds an Ethereum Node Record (ENR) using the provided network configuration and key.
pub fn build_enr(config: &P2PNetworkConfig, combined_key: &CombinedKey) -> Result<Enr> {
    enr::Enr::builder()
        .ip4(config.ipv4)
        .udp4(config.udpv4)
        .tcp4(config.tcpv4)
        .build(combined_key)
        .map_err(|e| anyhow!("Failed to build ENR: {}", e))
}

/// Trait to convert an ENR to a libp2p `PeerId`.
pub trait EnrAsPeerId {
    fn as_peer_id(&self) -> Result<PeerId>;
}

impl EnrAsPeerId for Enr {
    fn as_peer_id(&self) -> Result<PeerId> {
        let public_key = self.public_key();

        match public_key {
            CombinedPublicKey::Secp256k1(pk) => {
                let libp2p_pk = secp256k1::PublicKey::try_from_bytes(&pk.to_sec1_bytes())
                    .map_err(|_| anyhow!("Invalid Secp256k1 public key"))?;
                Ok(PeerId::from_public_key(&libp2p_pk.into()))
            }
            CombinedPublicKey::Ed25519(pk) => {
                let libp2p_pk = ed25519::PublicKey::try_from_bytes(&pk.to_bytes())
                    .map_err(|_| anyhow!("Invalid Ed25519 public key"))?;
                Ok(PeerId::from_public_key(&libp2p_pk.into()))
            }
        }
    }
}
