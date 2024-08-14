use super::block::Block;
use super::block_proposed::BlockProposed;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum NodeMessage {
    BlockProposed(BlockProposed),
    P2P(String),
    BlockPerconfirmation(Block),
}

impl From<NodeMessage> for Vec<u8> {
    fn from(val: NodeMessage) -> Self {
        bincode::serialize(&val).expect("Serialization failed")
    }
}

impl From<Vec<u8>> for NodeMessage {
    fn from(bytes: Vec<u8>) -> Self {
        bincode::deserialize(&bytes).expect("Deserialization failed")
    }
}
