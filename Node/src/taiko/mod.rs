pub struct Taiko {}

impl Taiko {
    pub fn new() -> Self {
        Self {}
    }

    pub fn get_pending_l2_tx_lists(&self) {
        tracing::debug!("Getting L2 tx lists");
    }

    pub fn submit_new_l2_blocks(&self) {
        tracing::debug!("Submitting new L2 blocks");
    }
}
