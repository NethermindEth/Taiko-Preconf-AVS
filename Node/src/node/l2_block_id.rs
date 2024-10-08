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

    pub fn update(&self, block_id: u64) {
        let mut current_block_id = self.block_id.load(Ordering::Acquire);

        while block_id > current_block_id {
            match self.block_id.compare_exchange(
                current_block_id,
                block_id,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(previous) => current_block_id = previous,
            }
        }
    }

    pub fn next(&self, block_id: u64) -> u64 {
        let mut current_block_id = self.block_id.load(Ordering::Acquire);
        let mut new_block_id = std::cmp::max(block_id + 1, current_block_id + 1);
        while new_block_id > current_block_id {
            match self.block_id.compare_exchange(
                current_block_id,
                new_block_id,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(previous) => {
                    current_block_id = previous;
                    new_block_id = std::cmp::max(new_block_id, current_block_id + 1);
                }
            }
        }
        new_block_id
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
}
