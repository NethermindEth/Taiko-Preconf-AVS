pub mod discovery;
pub mod enr;
pub mod network;
pub mod peer_manager;

pub fn generate_secp256k1() -> libp2p::identity::Keypair {
    libp2p::identity::Keypair::generate_secp256k1()
}
