pub type PreconferAddress = [u8; 20];
pub type ECDSASignature = [u8; 65]; // ECDSA 65 bytes signature
pub type BLSCompressedPublicKey = [u8; 48];

pub type Slot = u64;
pub type Epoch = u64;

// TODO for future usage
// pub type BLSUncompressedPublicKey = [u8; 96];
// pub type BLSSignature = [u8; 96];
