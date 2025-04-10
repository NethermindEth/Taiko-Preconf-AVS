
struct Metrics {
    pub preconfer_eth_balance: prometheus::Gauge,
    pub preconfer_taiko_balance: prometheus::Histogram,
    pub errors_counter: prometheus::Gauge,
}

impl Metrics {
    pub fn new() -> Self {
        let counter = prometheus::IntCounter::new("counter", "A counter").unwrap();
        let histogram = prometheus::Histogram::new("histogram", "A histogram").unwrap();
        let gauge = prometheus::Gauge::new("gauge", "A gauge").unwrap();
        Self { counter, histogram, gauge }
    }
}