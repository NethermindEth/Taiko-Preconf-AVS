use prometheus::{Counter, Encoder, Gauge, Histogram, HistogramOpts, Registry, TextEncoder};
use tracing::error;

pub struct Metrics {
    preconfer_eth_balance: Gauge,
    preconfer_taiko_balance: Gauge,
    blocks_preconfirmed: Counter,
    blocks_reanchored: Counter,
    batch_recovered: Counter,
    batch_sent: Counter,
    batch_confirmed: Counter,
    batch_propose_tries: Histogram,
    batch_block_count: Histogram,
    batch_blob_size: Histogram,
    registry: Registry,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let preconfer_eth_balance = Gauge::new(
            "preconfer_eth_balance",
            "Ethereum balance of the preconfer wallet",
        )
        .expect("Failed to create preconfer_eth_balance gauge");

        let preconfer_taiko_balance = Gauge::new(
            "preconfer_taiko_balance",
            "TAIKO balance of the preconfer wallet",
        )
        .expect("Failed to create preconfer_taiko_balance gauge");

        if let Err(err) = registry.register(Box::new(preconfer_eth_balance.clone())) {
            error!("Error: Failed to register preconfer_eth_balance: {}", err);
        }

        if let Err(err) = registry.register(Box::new(preconfer_taiko_balance.clone())) {
            error!("Error: Failed to register preconfer_taiko_balance: {}", err);
        }

        let blocks_preconfirmed = Counter::new(
            "blocks_preconfirmed",
            "Number of blocks preconfirmed by the node",
        )
        .expect("Failed to create blocks_preconfirmed counter");

        if let Err(err) = registry.register(Box::new(blocks_preconfirmed.clone())) {
            error!("Error: Failed to register blocks_preconfirmed: {}", err);
        }

        let blocks_reanchored = Counter::new(
            "blocks_reanchored",
            "Number of blocks reanchored by the node",
        )
        .expect("Failed to create blocks_reanchored counter");

        if let Err(err) = registry.register(Box::new(blocks_reanchored.clone())) {
            error!("Error: Failed to register blocks_reanchored: {}", err);
        }

        let batch_recovered =
            Counter::new("batch_recovered", "Number of batches recovered by the node")
                .expect("Failed to create batch_recovered counter");

        if let Err(err) = registry.register(Box::new(batch_recovered.clone())) {
            error!("Error: Failed to register batch_recovered: {}", err);
        }

        let batch_sent = Counter::new("batch_proposed", "Number of batches proposed by the node")
            .expect("Failed to create batch_proposed counter");

        if let Err(err) = registry.register(Box::new(batch_sent.clone())) {
            error!("Error: Failed to register batch_proposed: {}", err);
        }

        let batch_confirmed = Counter::new("batch_confirmed", "Number of batches landed on L1")
            .expect("Failed to create batch_confirmed counter");

        if let Err(err) = registry.register(Box::new(batch_confirmed.clone())) {
            error!("Error: Failed to register batch_confirmed: {}", err);
        }

        let opts = HistogramOpts::new("batch_propose_tries", "Number of tries to propose a batch")
            .buckets(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let batch_propose_tries = match Histogram::with_opts(opts) {
            Ok(histogram) => histogram,
            Err(err) => panic!("Failed to create batch_propose_tries histogram: {}", err),
        };

        if let Err(err) = registry.register(Box::new(batch_propose_tries.clone())) {
            error!("Error: Failed to register batch_propose_tries: {}", err);
        }

        let opts =
            HistogramOpts::new("batch_block_count", "Number of blocks in a batch").buckets(vec![
                76.0, 152.0, 228.0, 304.0, 380.0, 456.0, 532.0, 608.0, 684.0, 768.0,
            ]);
        let batch_block_count = match Histogram::with_opts(opts) {
            Ok(histogram) => histogram,
            Err(err) => panic!("Failed to create batch_block_count histogram: {}", err),
        };

        if let Err(err) = registry.register(Box::new(batch_block_count.clone())) {
            error!("Error: Failed to register batch_block_count: {}", err);
        }

        let opts = HistogramOpts::new("batch_blob_size", "Size of a batch's blob in bytes")
            .buckets(vec![
                13004.0, 26008.0, 39012.0, 52016.0, 65020.0, 78024.0, 91028.0, 104032.0, 117036.0,
                130044.0,
            ]);
        let batch_blob_size = match Histogram::with_opts(opts) {
            Ok(histogram) => histogram,
            Err(err) => panic!("Failed to create batch_blob_size histogram: {}", err),
        };

        if let Err(err) = registry.register(Box::new(batch_blob_size.clone())) {
            error!("Error: Failed to register batch_blob_size: {}", err);
        }

        Self {
            preconfer_eth_balance,
            preconfer_taiko_balance,
            blocks_preconfirmed,
            blocks_reanchored,
            batch_recovered,
            batch_sent,
            batch_confirmed,
            batch_propose_tries,
            batch_block_count,
            batch_blob_size,
            registry,
        }
    }

    pub fn set_preconfer_eth_balance(&self, balance: alloy::primitives::U256) {
        self.preconfer_eth_balance
            .set(Metrics::u256_to_f64(balance));
    }

    pub fn set_preconfer_taiko_balance(&self, balance: alloy::primitives::U256) {
        self.preconfer_taiko_balance
            .set(Metrics::u256_to_f64(balance));
    }

    pub fn inc_blocks_preconfirmed(&self) {
        self.blocks_preconfirmed.inc();
    }

    pub fn inc_by_blocks_reanchored(&self, value: u64) {
        self.blocks_reanchored.inc_by(value as f64);
    }

    pub fn inc_by_batch_recovered(&self, value: u64) {
        self.batch_recovered.inc_by(value as f64);
    }

    pub fn inc_batch_sent(&self) {
        self.batch_sent.inc();
    }

    pub fn inc_batch_confirmed(&self) {
        self.batch_confirmed.inc();
    }

    pub fn observe_batch_propose_tries(&self, tries: u64) {
        self.batch_propose_tries.observe(tries as f64);
    }

    pub fn observe_batch_info(&self, block_count: u64, blob_size: u64) {
        self.batch_block_count.observe(block_count as f64);
        self.batch_blob_size.observe(blob_size as f64);
    }

    fn u256_to_f64(balance: alloy::primitives::U256) -> f64 {
        let balance_str = balance.to_string();
        let len = balance_str.len();

        if len < 14 {
            return 0f64;
        }

        let mut result = balance_str.clone();
        if len <= 18 {
            result = format!("{:019}", balance_str);
        }

        result.insert(len - 18, '.');
        let result = result.split_at(len - 13).0.to_string();

        match result.parse::<f64>() {
            Ok(v) => v,
            Err(_) => 0f64,
        }
    }

    pub fn gather(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}
