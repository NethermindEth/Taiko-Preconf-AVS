use crate::taiko::taiko_blob_coder::{TaikoBlobCoder, MAX_BLOB_DATA_SIZE};
use alloy::{
    consensus::{Blob, BlobTransactionSidecar, Bytes48, EnvKzgSettings},
    eips::eip4844::BYTES_PER_BLOB,
};

use c_kzg::{KzgCommitment, KzgProof};

pub fn build_taiko_blob_sidecar(data: &[u8]) -> BlobTransactionSidecar {
    // Split to blob chunks
    let chunks: Vec<&[u8]> = data.chunks(MAX_BLOB_DATA_SIZE).collect();

    let mut blobs: Vec<Blob> = Vec::with_capacity(chunks.len());
    let mut commitments: Vec<Bytes48> = Vec::with_capacity(chunks.len());
    let mut proofs: Vec<Bytes48> = Vec::with_capacity(chunks.len());

    for raw_data_blob in chunks {
        blobs.push(Blob::new([0u8; BYTES_PER_BLOB]));
        // Encode blob data
        let mut coder = TaikoBlobCoder::new();
        let encoded_blob: Blob = coder.encode_blob( raw_data_blob);

        // SAFETY: same size
        let blob = unsafe { core::mem::transmute::<&Blob, &c_kzg::Blob>(&encoded_blob) };
        let kzg_settings = EnvKzgSettings::Default.get();
        let commitment = KzgCommitment::blob_to_kzg_commitment(blob, kzg_settings).unwrap();
        let proof = KzgProof::compute_blob_kzg_proof(blob, &commitment.to_bytes(), kzg_settings).unwrap();

        blobs.push(encoded_blob);
        // SAFETY: same size
        unsafe {

            commitments
                .push(core::mem::transmute::<c_kzg::Bytes48, Bytes48>(commitment.to_bytes()));
            proofs.push(core::mem::transmute::<c_kzg::Bytes48, Bytes48>(proof.to_bytes()));
        }
    }

    BlobTransactionSidecar {
        blobs,
        commitments,
        proofs,
    }
}
