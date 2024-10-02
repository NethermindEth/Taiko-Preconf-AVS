pub struct PreconfirmationHelper {
    nonce: u64,
}

impl PreconfirmationHelper {
    pub fn new() -> Self {
        Self { nonce: 0 }
    }

    pub fn init(&mut self, nonce: u64) {
        self.nonce = nonce;
    }

    pub fn get_next_nonce(&mut self) -> u64 {
        let nonce = self.nonce;
        self.nonce += 1;
        nonce
    }

    pub fn increment_nonce(&mut self) {
        self.nonce += 1;
    }
}
