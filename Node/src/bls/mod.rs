use bls::types::{G1AffinePoint, G2AffinePoint, PublicKey, SecretKey, Signature};
use bls_on_arkworks as bls;

pub struct BLSService {
    pk: PublicKey,
    sk: SecretKey,
}

impl BLSService {
    pub fn new(pk: &str) -> Self {
        let pk_bytes = alloy::hex::decode(pk).unwrap();
        let sk = bls::os2ip(&pk_bytes);
        let pk = bls::sk_to_pk(sk);

        Self { pk, sk }
    }

    pub fn sign(&self, message: &Vec<u8>, dst: &Vec<u8>) -> Signature {
        bls::sign(self.sk, message, dst).unwrap()
    }

    #[allow(dead_code)]
    // TODO: used in AddValidator call
    pub fn sign_as_point(&self, message: &Vec<u8>, dst: &Vec<u8>) -> G2AffinePoint {
        let sign = self.sign(message, dst);
        bls::signature_to_point(&sign).unwrap()
    }

    #[allow(dead_code)]
    // TODO: used in AddValidator call
    pub fn get_public_key(&self) -> G1AffinePoint {
        bls::pubkey_to_point(&self.pk).unwrap()
    }
}
