pub struct Block {
    pub tx_list_hash: [u8; 32],
    pub signature: [u8; 96], // BLS 96 bytes signature
}
