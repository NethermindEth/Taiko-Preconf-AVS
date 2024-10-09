use std::sync::atomic::{AtomicU64, Ordering};

pub struct L2BlockId {
    block_id: AtomicU64,
}

impl L2BlockId {
    pub fn new() -> Self {
        Self {
            block_id: AtomicU64::new(0),
        }
    }

    // Update self.block_id with the maximum value between self.block_id and block_id
    pub fn update(&self, block_id: u64) {
        self.block_id.fetch_max(block_id, Ordering::AcqRel);
    }

    // Returns the next block ID
    // The next block ID is computed as the maximum value between current_block_id + 1 and new_block_id + 1
    pub fn next(&self, block_id: u64) -> u64 {
        // Get the current value of current_block_id
        let mut current_block_id = self.block_id.load(Ordering::Acquire);
        // Initialize new_block_id
        let mut new_block_id = block_id + 1;
    
        loop {
            // Get next block ID 
            // It is the maximum value between current_block_id + 1 and new_block_id + 1
            new_block_id = new_block_id.max(current_block_id + 1);

            // Attempt to update the block ID using a compare-exchange operation
            match self.block_id.compare_exchange(
                current_block_id,
                new_block_id,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => return new_block_id, // Return immediately on success
                Err(previous) => {
                    current_block_id = previous;
                    // new_block_id gets recalculated at the start of the loop
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next() {
        let l2_block_id = L2BlockId::new();
        
        assert_eq!(l2_block_id.next(1), 2);
        assert_eq!(l2_block_id.next(0), 3);
        assert_eq!(l2_block_id.next(1), 4);
        assert_eq!(l2_block_id.next(7), 8);
        assert_eq!(l2_block_id.next(8), 9);
        assert_eq!(l2_block_id.next(4), 10);
    }

    #[test]
    fn test_update() {
        let l2_block_id = L2BlockId::new();
        
        l2_block_id.update(1);
        assert_eq!(l2_block_id.block_id.load(Ordering::SeqCst), 1);
        l2_block_id.update(10);
        assert_eq!(l2_block_id.block_id.load(Ordering::SeqCst), 10);
        l2_block_id.update(5);
        assert_eq!(l2_block_id.block_id.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_update_next() {
        let l2_block_id = L2BlockId::new();
        
        l2_block_id.update(1);
        assert_eq!(l2_block_id.block_id.load(Ordering::SeqCst), 1);
        l2_block_id.update(10);
        assert_eq!(l2_block_id.block_id.load(Ordering::SeqCst), 10);
        assert_eq!(l2_block_id.next(0), 11);
        assert_eq!(l2_block_id.next(12), 13);
        l2_block_id.update(5);
        assert_eq!(l2_block_id.block_id.load(Ordering::SeqCst), 13);
    }

}
