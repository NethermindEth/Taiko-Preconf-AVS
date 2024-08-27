pub struct PreconfirmationHelper {
    nonce: u64,
    final_slot_perconfirmation_count: u8,
}

impl PreconfirmationHelper {
    pub fn new() -> Self {
        Self {
            nonce: 0,
            final_slot_perconfirmation_count: 0,
        }
    }

    pub fn init(&mut self, nonce: u64) {
        self.nonce = nonce;
        self.final_slot_perconfirmation_count = 0;
    }

    pub fn get_next_nonce(&mut self) -> u64 {
        let nonce = self.nonce;
        self.nonce += 1;
        nonce
    }

    pub fn increment_final_slot_perconfirmation(&mut self) {
        self.final_slot_perconfirmation_count += 1;
    }

    pub fn is_last_final_slot_perconfirmation(&self) -> bool {
        self.final_slot_perconfirmation_count >= 3
    }
}
