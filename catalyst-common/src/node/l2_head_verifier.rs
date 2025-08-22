use alloy::primitives::B256;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
struct L2HeadStatus {
    number: u64,
    hash: B256,
}

pub struct L2HeadVerifier {
    head: Arc<Mutex<L2HeadStatus>>,
}

impl L2HeadVerifier {
    pub fn new() -> Self {
        let head = Arc::new(Mutex::new(L2HeadStatus {
            number: 0,
            hash: B256::ZERO,
        }));
        Self { head }
    }

    pub async fn set(&self, number: u64, hash: B256) {
        let mut head = self.head.lock().await;
        head.number = number;
        head.hash = hash;
    }

    pub async fn verify(&self, number: u64, hash: &B256) -> bool {
        let head = self.head.lock().await;
        number == head.number && *hash == head.hash
    }

    pub async fn verify_next_and_set(&self, number: u64, hash: B256, parent_hash: B256) -> bool {
        let mut head = self.head.lock().await;
        if number == head.number + 1 && parent_hash == head.hash {
            head.number = number;
            head.hash = hash;
            return true;
        }
        false
    }

    pub async fn log_error(&self) {
        let head = self.head.lock().await;
        tracing::error!(
            "📕 L2HeadStatus number: {} hash: {}",
            head.number,
            head.hash
        );
    }
}
