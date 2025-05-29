use alloy::{
    consensus::{Blob, SidecarCoder, utils::WholeFe},
    eips::eip4844::{BYTES_PER_BLOB, FIELD_ELEMENT_BYTES_USIZE, builder::PartialSidecar},
};
use anyhow::Error;

const BLOB_SIZE: usize = 4096 * 32; // byte size of a blob. 4096 field elements * 32 bytes
const DATA_LENGTH_SIZE: usize = 4; // number of bytes to store the data length
const DATA_WRITTEN_PER_ROUND: usize = (4 * 31 + 3); // number of bytes written per encode/decode round
const FE_WRITTEN_PER_ROUND: usize = 4; // number of field elements written per encode/decode round
const ROUNDS: usize = 1024; // number of encode/decode rounds
pub const MAX_BLOB_DATA_SIZE: usize = DATA_WRITTEN_PER_ROUND * ROUNDS - DATA_LENGTH_SIZE; // maximum number of bytes that can be encoded in the blob
const ENCODING_VERSION: u8 = 0;
const VERSION_OFFSET: usize = 1; // offset of the version byte in the blob encoding

pub struct TaikoBlobCoder {
    read_offset: usize,
    blob_offset: usize,
    blob: [u8; BYTES_PER_BLOB],
}

impl TaikoBlobCoder {
    pub fn encode_blob(data: &[u8]) -> Result<Blob, Error> {
        TaikoBlobCoder::new().encode_data(data)
    }

    fn new() -> Self {
        Self {
            read_offset: 0,
            blob_offset: 0,
            blob: [0u8; BYTES_PER_BLOB],
        }
    }

    fn read1(&mut self, data: &[u8]) -> u8 {
        if self.read_offset >= data.len() {
            return 0;
        }
        let r = data[self.read_offset];
        self.read_offset += 1;
        r
    }

    fn write_fe(&mut self, first_byte: u8, data31: &[u8]) {
        self.blob[self.blob_offset] = first_byte;
        self.blob[self.blob_offset + 1..self.blob_offset + 32].copy_from_slice(data31);
        self.blob_offset += 32;
    }

    fn read31(&mut self, data: &[u8]) -> [u8; 31] {
        let mut result = [0u8; 31];
        let available_bytes = (data.len() - self.read_offset).min(31);
        result[..available_bytes]
            .copy_from_slice(&data[self.read_offset..self.read_offset + available_bytes]);
        self.read_offset += available_bytes;
        result
    }

    // Encodes the given input data into this blob. The encoding scheme is as follows:
    //
    // In each round we perform 7 reads of input of lengths (31,1,31,1,31,1,31) bytes respectively for
    // a total of 127 bytes. This data is encoded into the next 4 field elements of the output by
    // placing each of the 4x31 byte chunks into bytes [1:32] of its respective field element. The
    // three single byte chunks (24 bits) are split into 4x6-bit chunks, each of which is written into
    // the top most byte of its respective field element, leaving the top 2 bits of each field element
    // empty to avoid modulus overflow.  This process is repeated for up to 1024 rounds until all data
    // is encoded.
    //
    // For only the very first output field, bytes [1:5] are used to encode the version and the length
    // of the data.
    // Refer to https://github.com/ethereum-optimism/optimism/blob/develop/op-service/eth/blob.go#L92
    fn encode_data(&mut self, data: &[u8]) -> Result<Blob, Error> {
        if data.is_empty() {
            return Err(anyhow::anyhow!("Cannot encode empty data"));
        }
        if data.len() > MAX_BLOB_DATA_SIZE {
            return Err(anyhow::anyhow!("Data is bigger than MAX_BLOB_DATA_SIZE"));
        }

        // Init read buffer
        let mut buf31 = [0u8; 31];

        for round in 0..ROUNDS {
            if self.read_offset >= data.len() {
                break;
            }

            // First FE
            if round == 0 {
                // special case for the zeroth round
                buf31[0] = ENCODING_VERSION;
                let ilen = u32::try_from(data.len())?;
                buf31[1..4].copy_from_slice(&ilen.to_be_bytes()[1..]);
                let to_read = data.len().min(27); // 27 = 31 - 4
                buf31[4..4 + to_read].clone_from_slice(&data[..to_read]);
                self.read_offset += to_read;
            } else {
                buf31 = self.read31(data);
            }

            let x = self.read1(data);
            self.write_fe(x & 0b0011_1111, &buf31);

            // Second FE
            buf31 = self.read31(data);
            let y = self.read1(data);
            self.write_fe((y & 0b0000_1111) | ((x & 0b1100_0000) >> 2), &buf31);

            // Third FE
            buf31 = self.read31(data);
            let z = self.read1(data);
            self.write_fe(z & 0b0011_1111, &buf31);

            // Fourth FE
            buf31 = self.read31(data);
            self.write_fe(((z & 0b1100_0000) >> 2) | ((y & 0b1111_0000) >> 4), &buf31);
        }

        if self.read_offset < data.len() {
            return Err(anyhow::anyhow!(
                "Expected to fit data but failed, read offset: {}, data len: {}",
                self.read_offset,
                data.len()
            ));
        }

        Ok(Blob::new(self.blob))
    }
}
