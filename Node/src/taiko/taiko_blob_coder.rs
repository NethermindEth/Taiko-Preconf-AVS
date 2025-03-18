use alloy::{
    consensus::{utils::WholeFe, Blob, SidecarCoder},
    eips::eip4844::{builder::PartialSidecar, BYTES_PER_BLOB, FIELD_ELEMENT_BYTES_USIZE},
};

const BLOB_SIZE: usize = 4096 * 32; // byte size of a blob. 4096 field elements * 32 bytes
const DATA_LENGTH_SIZE: usize = 4; // number of bytes to store the data length
const DATA_WRITEN_PER_ROUND: usize = (4 * 31 + 3); // number of bytes written per encode/decode round
const FE_WRITEN_PER_ROUND: usize = 4; // number of field elements written per encode/decode round
const ROUNDS: usize = 1024; // number of encode/decode rounds
pub const MAX_BLOB_DATA_SIZE: usize = DATA_WRITEN_PER_ROUND * ROUNDS - DATA_LENGTH_SIZE; // maximum number of bytes that can be encoded in the blob
const ENCODING_VERSION: u8 = 0;
const VERSION_OFFSET: usize = 1; // offset of the version byte in the blob encoding

#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct TaikoBlobCoder {
    read_offset: usize,
    blob_offset: usize,
}

impl TaikoBlobCoder {
    fn read1(&mut self, data: &[u8]) -> u8 {
        if self.read_offset >= data.len() {
            return 0;
        }
        let r = data[self.read_offset];
        self.read_offset += 1;
        r
    }

    fn build_fe(&mut self, first_byte: u8, data31: &[u8]) -> [u8; 32] {
        let mut buf32 = [0u8; 32];
        buf32[0] = first_byte;
        buf32[1..].copy_from_slice(data31);
        buf32
    }

    fn read31(&mut self, data: &[u8]) -> [u8; 31] {
        let mut result = [0u8; 31];
        let available_bytes = (data.len() - self.read_offset).min(31);
        result[..available_bytes]
            .copy_from_slice(&data[self.read_offset..self.read_offset + available_bytes]);
        self.read_offset += available_bytes;
        result
    }
}

impl TaikoBlobCoder {
    pub fn new() -> Self {
        Self {
            read_offset: 0,
            blob_offset: 0,
        }
    }

    pub fn encode_blob(&mut self, data: &[u8]) -> Blob {
        if data.is_empty() {
            return Blob::new([0u8; BYTES_PER_BLOB]); //TODO
        }
        if data.len() > MAX_BLOB_DATA_SIZE {
            panic!("You use coder incorrectly. It can encode only one blob at a time and you can't add extra data.");
        }

        // Init read offset
        self.read_offset = 0;

        // Init blob offset
        self.blob_offset = 0;

        // Init result
        let mut blob = [0u8; BYTES_PER_BLOB];

        // Init read buffer
        let mut buf31 = [0u8; 31];

        for round in 0..ROUNDS {
            if self.read_offset >= data.len() {
                break;
            }

            if round == 0 {
                // leave the firs u8 empty for future setup
                buf31[0] = ENCODING_VERSION;
                let ilen = data.len() as u32;
                buf31[1..4].copy_from_slice(&ilen.to_be_bytes()[1..]);
                let to_read = data.len().min(27); // 27 = 31 - 4
                buf31[4..4 + to_read].clone_from_slice(&data[..to_read]);
                self.read_offset += to_read;
            } else {
                buf31 = self.read31(data);
            }

            let x = self.read1(data);
            blob[self.blob_offset..self.blob_offset + 32]
                .clone_from_slice(&self.build_fe(x & 0b0011_1111, &buf31));
            self.blob_offset += 32;

            buf31 = self.read31(data);
            let y = self.read1(data);
            blob[self.blob_offset..self.blob_offset + 32].clone_from_slice(
                &self.build_fe((y & 0b0000_1111) | ((x & 0b1100_0000) >> 2), &buf31),
            );
            self.blob_offset += 32;

            buf31 = self.read31(data);
            let z = self.read1(data);
            blob[self.blob_offset..self.blob_offset + 32]
                .clone_from_slice(&self.build_fe(z & 0b0011_1111, &buf31));
            self.blob_offset += 32;

            buf31 = self.read31(data);
            blob[self.blob_offset..self.blob_offset + 32].clone_from_slice(
                &self.build_fe(((z & 0b1100_0000) >> 2) | ((y & 0b1111_0000) >> 4), &buf31),
            );
            self.blob_offset += 32;
        }

        if self.read_offset < data.len() {
            panic!(
                "Expected to fit data but failed, read offset: {}, data len: {}",
                self.read_offset,
                data.len()
            );
        }

        Blob::new(blob)
    }
}
