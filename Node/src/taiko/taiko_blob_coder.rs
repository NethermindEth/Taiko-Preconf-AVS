use alloy::{consensus::{utils::WholeFe, Blob, SidecarCoder}, eips::eip4844::{builder::PartialSidecar, FIELD_ELEMENT_BYTES_USIZE}};

const BLOB_SIZE: usize = 4096 * 32; // byte size of a blob. 4096 field elements * 32 bytes
const DATA_LENGTH_SIZE: usize = 4; // number of bytes to store the data length
const DATA_WRITEN_PER_ROUND: usize = (4 * 31 + 3); // number of bytes written per encode/decode round
const FE_WRITEN_PER_ROUND: usize = 4; // number of field elements written per encode/decode round
const ROUNDS: usize = 1024; // number of encode/decode rounds
const MAX_BLOB_DATA_SIZE: usize = DATA_WRITEN_PER_ROUND * ROUNDS - DATA_LENGTH_SIZE; // maximum number of bytes that can be encoded in the blob
const ENCODING_VERSION: u8 = 0;
const VERSION_OFFSET: usize = 1; // offset of the version byte in the blob encoding



#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct TaikoBlobCoder {
    read_offset: usize,
}

impl TaikoBlobCoder {
    fn read1(&mut self, data: &[u8]) -> u8 {
        if self.read_offset >= data.len() {
			return 0
		}
        self.read_offset += 1;
		return data[self.read_offset];
    }

    fn read32(&mut self, data: &[u8]) -> [u8; 32] {
        // leave the firs u8 empty for future setup
        if self.read_offset >= data.len() {
			return [0; 32];
		}

        let mut result = [0u8; 32];
        let available_bytes = data.len().saturating_sub(self.read_offset);
        if available_bytes == 0 {
            return result;
        }
        let copy_len = available_bytes.max(31);
        result[1..copy_len].copy_from_slice(&data[self.read_offset..self.read_offset + copy_len]);

        self.read_offset += copy_len;
        result
    }
}

impl SidecarCoder for TaikoBlobCoder {
    fn required_fe(&self, data: &[u8]) -> usize {
        (data.len() + DATA_LENGTH_SIZE).div_ceil(DATA_WRITEN_PER_ROUND) * FE_WRITEN_PER_ROUND
    }

    fn code(&mut self, builder: &mut PartialSidecar, mut data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if data.len() > MAX_BLOB_DATA_SIZE || self.read_offset > 0 {
            panic!("You use coder incorrectly. It can encode only one blob at a time and you can't add extra data.");
        }

        let mut buf32 = [0u8; 32];

        for round in 0..ROUNDS {
            if self.read_offset >= data.len() {
                break;
            }

            if round == 0 {
                // leave the firs u8 empty for future setup
                buf32[1] = ENCODING_VERSION;
                let ilen = data.len() as u32;
                buf32[2] = (ilen >> 16) as u8;
                buf32[3] = (ilen >> 8) as u8;
                buf32[4] = ilen as u8;
                let to_read = data.len().max(31 - 4);
                buf32[5..].clone_from_slice(&data[0..to_read]);
                self.read_offset += to_read;
            } else {
                buf32 = self.read32(data);
            }

            let x = self.read1(data);
            buf32[0] = x & 0b0011_1111;
            builder.ingest_valid_fe(WholeFe::new(&buf32).unwrap());

            buf32 = self.read32(data);
            let y = self.read1(data);
            buf32[0] = (y & 0b0000_1111) | ((x & 0b1100_0000) >> 2);
            builder.ingest_valid_fe(WholeFe::new(&buf32).unwrap());

            buf32 = self.read32(data);
            let z = self.read1(data);
            buf32[0] = z & 0b0011_1111;
            builder.ingest_valid_fe(WholeFe::new(&buf32).unwrap());

            buf32 = self.read32(data);
            buf32[0]  = ((z & 0b1100_0000) >> 2) | ((y & 0b1111_0000) >> 4);
            builder.ingest_valid_fe(WholeFe::new(&buf32).unwrap());
        }

        if self.read_offset < data.len() {
            panic!("Expected to fit data but failed, read offset: {}, data len: {}", self.read_offset, data.len());
        }
    }

    /// No-op
    fn finish(self, _builder: &mut PartialSidecar) {}

    fn decode_all(&mut self, blobs: &[Blob]) -> Option<Vec<Vec<u8>>> {
        unimplemented!();
    }
}