use crate::taiko::taiko_blob_coder::{TaikoBlobCoder, MAX_BLOB_DATA_SIZE};
use alloy::{
    consensus::{Blob, BlobTransactionSidecar, Bytes48, EnvKzgSettings},
    eips::eip4844::BYTES_PER_BLOB, primitives::FixedBytes,
};

pub fn build_taiko_blob_sidecar(data: &[u8]) -> BlobTransactionSidecar {
    // Split to blob chunks
    let chunks: Vec<&[u8]> = data.chunks(MAX_BLOB_DATA_SIZE).collect();

    let mut blobs: Vec<Blob> = Vec::with_capacity(chunks.len());
    let mut commitments: Vec<Bytes48> = Vec::with_capacity(chunks.len());
    let mut proofs: Vec<Bytes48> = Vec::with_capacity(chunks.len());

    for raw_data_blob in chunks {
        // Encode blob data
        let mut coder = TaikoBlobCoder::new();
        let encoded_blob: Blob = coder.encode_blob( raw_data_blob);
        // Compute commitment and proof
        let kzg_settings = EnvKzgSettings::Default.get();
        let commitment = ethereum_consensus::crypto::kzg::blob_to_kzg_commitment(encoded_blob, kzg_settings).unwrap();
        let proof = ethereum_consensus::crypto::kzg::compute_blob_kzg_proof(encoded_blob, &commitment, kzg_settings).unwrap();
        // Build sidecar
        blobs.push(encoded_blob);
        commitments.push(Bytes48::try_from(commitment.as_ref()).unwrap());
        proofs.push(Bytes48::try_from(proof.as_ref()).unwrap());
    }

    BlobTransactionSidecar {
        blobs,
        commitments,
        proofs,
    }
}

mod tests {
    use alloy::consensus::{SidecarBuilder, SimpleCoder};

    use super::*;

    #[test]
    fn test_build_taiko_blob_sidecar() {
        let data = vec![3u8; 200];
        let sidecar = build_taiko_blob_sidecar(&data);
        for s in sidecar.into_iter() {
            assert!(s.verify_blob_kzg_proof().is_ok());
        }
    }
}