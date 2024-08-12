use super::block_proposed::BlockProposed;

#[derive(Debug)]
pub enum NodeMessage {
    BlockProposed(BlockProposed),
    P2P(String),
}
