use crate::types::*;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

pub struct Keypair {
    pub signing_key: SigningKey,
    pub address: Address,
}

impl Keypair {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let address = Address::from_pubkey(&signing_key.verifying_key());
        Keypair { signing_key, address }
    }
}