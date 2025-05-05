use crate::utils::types::*;
use alloy::primitives::B256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreconfBlockRequestBody {
    pub executable_data: ExecutableData,
    pub end_of_sequencing: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableData {
    pub base_fee_per_gas: u64,
    pub block_number: u64,
    pub extra_data: String,
    pub fee_recipient: String,
    pub gas_limit: u64,
    pub parent_hash: String,
    pub timestamp: u64,
    pub transactions: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovePreconfBlockRequestBody {
    pub new_last_block_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaikoStatus {
    #[serde(rename = "highestUnsafeL2PayloadBlockID")]
    pub highest_unsafe_l2_payload_block_id: u64,
    #[serde(rename = "endOfSequencingMarkerReceived")]
    pub end_of_sequencing_marker_received: bool,
}
