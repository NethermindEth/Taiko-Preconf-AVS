use blst::min_pk::{PublicKey, SecretKey, Signature};

pub struct BLSService {
    pk: PublicKey,
    sk: SecretKey,
}

impl BLSService {
    pub fn new(pk: &str) -> Self {
        let sk = SecretKey::from_bytes(&hex::decode(pk).unwrap()).unwrap();
        let pk = sk.sk_to_pk();
        Self { pk, sk }
    }

    pub fn sign(&self, message: &[u8], dst: &[u8]) -> Signature {
        self.sk.sign(message, dst, &self.pk.to_bytes())
    }
}
