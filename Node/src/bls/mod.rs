use alloy::primitives::U256;
use anyhow::Error;
use bls::types::{G1AffinePoint, G2AffinePoint, PublicKey, SecretKey, Signature};
use bls_on_arkworks as bls;
use num_bigint::BigUint;
#[cfg(test)]
#[cfg(not(feature = "use_mock"))]
use rand_core::{OsRng, RngCore};

pub struct BLSService {
    pk: PublicKey,
    sk: SecretKey,
}

impl BLSService {
    pub fn new(private_key: &str) -> Result<Self, Error> {
        let pk_bytes = alloy::hex::decode(private_key)
            .map_err(|e| anyhow::anyhow!("BLSService: failed to decode private key: {}", e))?;
        let sk = bls::os2ip(&pk_bytes);
        let public_key = bls::sk_to_pk(sk);

        tracing::info!(
            "BLSService: public key: {}",
            hex::encode(public_key.clone())
        );

        Ok(Self { pk: public_key, sk })
    }

    #[cfg(test)]
    #[cfg(not(feature = "use_mock"))]
    pub fn generate_key() -> Self {
        let mut ikm = [0u8; 64];
        OsRng.fill_bytes(&mut ikm);

        let sk = bls::keygen(&ikm.to_vec());
        let pk = bls::sk_to_pk(sk);

        Self { pk, sk }
    }

    pub fn sign(&self, message: &Vec<u8>, dst: &Vec<u8>) -> Signature {
        bls::sign(self.sk, message, dst).unwrap()
    }

    pub fn sign_as_point(&self, message: &Vec<u8>, dst: &Vec<u8>) -> G2AffinePoint {
        let sign = self.sign(message, dst);
        bls::signature_to_point(&sign).unwrap()
    }

    pub fn biguint_to_u256_array(biguint: BigUint) -> [U256; 2] {
        let s = format!("{:0>96x}", biguint);
        let res1 = U256::from_str_radix(&s[0..32], 16).unwrap();
        let res2 = U256::from_str_radix(&s[32..96], 16).unwrap();

        [res1, res2]
    }

    #[cfg(test)]
    #[cfg(not(feature = "use_mock"))]
    pub fn get_public_key_compressed(&self) -> PublicKey {
        self.pk.clone()
    }

    pub fn get_public_key(&self) -> G1AffinePoint {
        bls::pubkey_to_point(&self.pk).unwrap()
    }
}
