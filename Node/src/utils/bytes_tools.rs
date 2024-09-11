use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;
use tiny_keccak::{Hasher, Keccak};

// Compress compresses the given tx_list bytes using zlib.
pub fn compress(tx_list: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(tx_list)?;
    encoder.finish()
}

pub fn hash_bytes_with_keccak(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut result = [0u8; 32];
    hasher.finalize(&mut result);
    result
}
