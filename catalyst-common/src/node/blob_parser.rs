use std::{sync::Arc, time::Duration};

use alloy::{eips::eip4844::kzg_to_versioned_hash, primitives::B256, rpc::types::Transaction};
use anyhow::Error;

use crate::shared::l2_tx_lists::uncompress_and_decode;
use crate::{
    ethereum_l1::{EthereumL1, extension::ELExtension},
    utils::blob::decode_blob,
};

pub async fn extract_transactions_from_blob<T: ELExtension>(
    ethereum_l1: Arc<EthereumL1<T>>,
    block: u64,
    blob_hash: Vec<B256>,
    tx_list_offset: u32,
    tx_list_size: u32,
) -> Result<Vec<Transaction>, Error> {
    let start = std::time::Instant::now();
    let v = blob_to_vec(ethereum_l1, block, blob_hash, tx_list_offset, tx_list_size).await?;
    tracing::debug!(
        "extract_transactions_from_blob: Blob conversion took {} ms",
        start.elapsed().as_millis()
    );
    let txs = uncompress_and_decode(v.as_slice())?;
    tracing::debug!(
        "extract_transactions_from_blob: Total with decompression and decoding took {} ms",
        start.elapsed().as_millis()
    );
    Ok(txs)
}

async fn blob_to_vec<T: ELExtension>(
    ethereum_l1: Arc<EthereumL1<T>>,
    block: u64,
    blob_hash: Vec<B256>,
    tx_list_offset: u32,
    tx_list_size: u32,
) -> Result<Vec<u8>, Error> {
    let timestamp = ethereum_l1
        .execution_layer
        .get_block_timestamp_by_number(block)
        .await?;
    let slot = ethereum_l1
        .slot_clock
        .slot_of(Duration::from_secs(timestamp))?;
    let sidecars = ethereum_l1.consensus_layer.get_blob_sidecars(slot).await?;

    let mut result: Vec<u8> = Vec::new();

    let sidecar_hashes: Vec<B256> = sidecars
        .data
        .iter()
        .map(|sidecar| kzg_to_versioned_hash(sidecar.kzg_commitment.as_slice()))
        .collect();

    for hash in blob_hash {
        for (i, sidecar) in sidecars.data.iter().enumerate() {
            if sidecar_hashes[i] == hash {
                let data = decode_blob(sidecar.blob.as_ref())?;
                result.extend(data);
                break;
            }
        }
    }

    let tx_list_left: usize = tx_list_offset.try_into()?;
    let tx_list_right: usize = tx_list_left + usize::try_from(tx_list_size)?;

    if tx_list_right > result.len() {
        return Err(anyhow::anyhow!(
            "Invalid tx list offset or size: tx_list_offset {} tx_list_size {} blob_data_size {}",
            tx_list_offset,
            tx_list_size,
            result.len()
        ));
    }

    let result = result[tx_list_left..tx_list_right].to_vec();

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        shared::l2_tx_lists::{
            decompose_pending_lists_json_from_geth, encode_and_compress, uncompress_and_decode,
        },
        utils::blob::build_blob_sidecar,
    };

    #[test]
    fn test_encode_and_decode_txs() {
        let str = r#"[{"BytesLength":220,"EstimatedGasUsed":0,"TxList":[{"accessList":[],"chainId":"0x28c59","gas":"0x33450","gasPrice":null,"hash":"0xf629a812d6aa6e9980dc4345f9cb922d3ebab9fb4cd37d2f5e5f39084c0edf3f","input":"0x","maxFeePerGas":"0x6fc23ac00","maxPriorityFeePerGas":"0x77359400","nonce":"0x2","r":"0xad528609b97f7ba8b8775aaf97d2229907ad2eac1b95f76fb567a3ae3bde46b9","s":"0x56e6fbe4b033c9c5d2098d21dfb0115a7ce7d5be760f947da30fee18c255546d","to":"0x5291a539174785fadc93effe9c9ceb7a54719ae4","type":"0x2","v":"0x1","value":"0x1550f7dca70000","yParity":"0x1"},{"accessList":[],"chainId":"0x28c59","gas":"0x33450","gasPrice":null,"hash":"0xca77045ed7340eaa0cc465f100c0470e162af9106acf56285729529ddee0e743","input":"0x","maxFeePerGas":"0x6fc23ac00","maxPriorityFeePerGas":"0x77359400","nonce":"0x3","r":"0xc79c04f7aa8d01eaa607ed9b69446fa8b3c3b9c0774d87ce7ed331df34bf8cc7","s":"0x19ba8dc8d6c6c3c3f69e54ea6fa61ec898deb1ec2ad412353756ea300aa1bc5a","to":"0x5291a539174785fadc93effe9c9ceb7a54719ae4","type":"0x2","v":"0x1","value":"0x1550f7dca70000","yParity":"0x1"}]}]"#;
        let value: serde_json::Value = serde_json::from_str(str).unwrap();
        let pending_lists = decompose_pending_lists_json_from_geth(value).unwrap();
        let txs = pending_lists[0].tx_list.clone();
        let compress = encode_and_compress(&txs).unwrap();
        let blob = build_blob_sidecar(&compress).unwrap();
        assert_eq!(blob.blobs.len(), 1);
        let blob_data = decode_blob(&blob.blobs[0]).unwrap();
        assert_eq!(blob_data, compress);
        let decoded_txs = uncompress_and_decode(&blob_data).unwrap();
        assert_eq!(decoded_txs, txs);
    }
}
