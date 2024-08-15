pub struct Block {
    pub commit_hash: [u8; 32],
    pub signature: [u8; 65], // ECDSA 65 bytes signature
}
