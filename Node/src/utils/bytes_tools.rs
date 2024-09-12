use flate2::read::ZlibDecoder;
use std::io::Read;
use tiny_keccak::{Hasher, Keccak};

// Decompress decompresses the given zlib-compressed bytes.
pub fn decompress_with_zlib(compressed: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = ZlibDecoder::new(compressed);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn hash_bytes_with_keccak(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut result = [0u8; 32];
    hasher.finalize(&mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use hex;
    use std::io::Write;

    pub fn compress(tx_list: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(tx_list)?;
        encoder.finish()
    }

    #[test]
    fn test_decompress() {
        let original_data = b"Hello, world! This is a test of the decompress function. Some more text to make it longer.";

        let compressed_data = compress(original_data).expect("Compression failed");
        println!("Compressed data (hex): {}", hex::encode(&compressed_data));

        let hex_encoded_from_go_lang = "789c04c0c10dc2301044d1563e77943ab8430351322116b607ed2e82f27937f5ee2b5f47df2f3cce96b464a594850fea14bb368f772893e333b76a9e0b770f311ca2f42bca8cf5255ad13d9f8ae51f0000ffffa0941ffd";
        let decoded_bytes_go_lang = hex::decode(hex_encoded_from_go_lang).expect("Decoding failed");

        println!("Decoded bytes: {}", hex::encode(&decoded_bytes_go_lang));

        let decompressed_go_lang =
            decompress_with_zlib(&decoded_bytes_go_lang).expect("Decompression failed");

        assert_eq!(decompressed_go_lang, original_data);

        println!(
            "Decompressed go lang data: {}",
            String::from_utf8_lossy(&decompressed_go_lang)
        );

        let decompressed_data =
            decompress_with_zlib(&compressed_data).expect("Decompression failed");

        assert_eq!(decompressed_data, original_data);
    }

    #[test]
    fn test_decompress_random_bytes() {
        // Create some sample data
        let original_data = hex::decode("50f400150851dca1d67a6d157cb7ef80c39652df2d97a4276ca0c2155e923500ee02f6eda749e3afa7862fd6009147420958590749548f5b220ac9f48347a06831422091711f8c9b6558b1d5af3e3aa509bf100af7fca94efd157e81c164a3693c528f1d").expect("Decoding failed");

        let compressed_data = compress(original_data.as_slice()).expect("Compression failed");
        let hex_encoded_from_go_lang = "789c0064009bff50f400150851dca1d67a6d157cb7ef80c39652df2d97a4276ca0c2155e923500ee02f6eda749e3afa7862fd6009147420958590749548f5b220ac9f48347a06831422091711f8c9b6558b1d5af3e3aa509bf100af7fca94efd157e81c164a3693c528f1d010000ffff1f452d9b";
        let decoded_bytes_go_lang = hex::decode(hex_encoded_from_go_lang).expect("Decoding failed");
        let decompressed_go_lang =
            decompress_with_zlib(&decoded_bytes_go_lang).expect("Decompression failed");

        assert_eq!(decompressed_go_lang, original_data);

        let decompressed_data =
            decompress_with_zlib(&compressed_data).expect("Decompression failed");
        assert_eq!(decompressed_data, original_data);
    }
}
