use super::block::Block;
use super::block_proposed::BlockProposed;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum NodeMessage {
    BlockProposed(BlockProposed),
    P2P(String),
    BlockPerconfirmation(Block),
}

impl Into<Vec<u8>> for NodeMessage {
    fn into(self) -> Vec<u8> {
        bincode::serialize(&self).expect("Serialization failed")
    }
}

impl From<Vec<u8>> for NodeMessage {
    fn from(bytes: Vec<u8>) -> Self {
        bincode::deserialize(&bytes).expect("Deserialization failed")
    }
}