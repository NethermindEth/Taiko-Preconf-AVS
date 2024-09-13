use tiny_keccak::{Hasher, Keccak};

pub fn hash_bytes_with_keccak(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut result = [0u8; 32];
    hasher.finalize(&mut result);
    result
}
