use crate::utils::types::*;
use alloy::primitives::B256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPreconfBlockRequestBody {
    pub executable_data: ExecutableData,
    pub signature: String,
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


