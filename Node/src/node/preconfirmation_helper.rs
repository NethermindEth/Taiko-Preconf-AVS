pub struct PreconfirmationHelper {
    nonce: u64,
    last_block_id: u64,
}

impl PreconfirmationHelper {
    pub fn new() -> Self {
        Self {
            nonce: 0,
            last_block_id: 0,
        }
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

    pub fn get_new_block_id(&mut self, parent_block_id: u64) -> u64 {
        let mut new_block_id = parent_block_id + 1;
        if self.last_block_id >= new_block_id {
            new_block_id = self.last_block_id + 1;
        }
        self.last_block_id = new_block_id;
        new_block_id
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_new_block_id() {
        let mut helper = PreconfirmationHelper::new();
        assert_eq!(helper.get_new_block_id(0), 1);
        assert_eq!(helper.get_new_block_id(0), 2);
        assert_eq!(helper.get_new_block_id(0), 3);
        assert_eq!(helper.get_new_block_id(0), 4);
        assert_eq!(helper.get_new_block_id(4), 5);
        assert_eq!(helper.get_new_block_id(4), 6);
        assert_eq!(helper.get_new_block_id(4), 7);
        assert_eq!(helper.get_new_block_id(4), 8);
    }
}