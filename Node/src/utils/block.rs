use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub tx_list_hash: [u8; 32],
    #[serde(with = "serde_bytes")]
    pub signature: [u8; 96], // BLS 96 bytes signature
}
