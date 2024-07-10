use anyhow::Error;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use secp256k1::{Message, Secp256k1, SecretKey};
use tiny_keccak::{Hasher, Keccak};
use bincode;

// ... existing code ...

impl RPCReplyL2TxLists {
    pub fn hash(&self) -> [u8; 32] {
        let serialized = bincode::serialize(self).expect("Serialization failed");
        let mut hasher = Keccak::v256();
        hasher.update(&serialized);
        let mut result = [0u8; 32];
        hasher.finalize(&mut result);
        result
    }

    pub fn sign(&self, private_key: &str) -> Result<secp256k1::Signature, Error> {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&hex::decode(private_key)?)?;
        let message = Message::from_slice(&self.hash())?;
        let signature = secp.sign_ecdsa(&message, &secret_key);
        Ok(signature)
    }
}

//TODO dokończyć implementację tego dla tx listy
// i jescze te dodatkowe pola umieścić, zastanowić się dalej jak z tx listą czy przekazywać też
// binarną wersję txlisty