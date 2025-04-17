use prometheus::{Encoder, Gauge, Registry, TextEncoder};
use tracing::error;

pub struct Metrics {
    preconfer_eth_balance: Gauge,
    preconfer_taiko_balance: Gauge,
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

        Self {
            preconfer_eth_balance,
            preconfer_taiko_balance,
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
