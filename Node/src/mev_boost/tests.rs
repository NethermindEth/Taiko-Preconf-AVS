#[cfg(test)]
mod tests {
    use crate::{bls::BLSService, mev_boost::MevBoost};
    use rand::Rng;
    use std::{process::Command, sync::Arc};

    // Helper function to check if the container is running
    async fn is_container_running(container_name: &str) -> bool {
        let output = Command::new("docker")
            .args(&["ps", "-q", "-f", &format!("name={}", container_name)])
            .output()
            .expect("Failed to check container status");

        !output.stdout.is_empty()
    }

    // Helper function to generate random vector with random size
    fn generate_random_vec_with_random_size(min_size: usize, max_size: usize) -> Vec<u8> {
        let mut rng = rand::thread_rng();
        let size = rng.gen_range(min_size..=max_size);
        (0..size).map(|_| rng.gen()).collect()
    }

    #[tokio::test]
    async fn test_mev_boost_mock() {
        if !is_container_running("mev-boost-mock").await {
            println!("Skipping test because mev-boost-mock container is not running.");
            return; // Skip the rest of the test
        }
        // Create a new BLSService with private key from Docker container
        let bls_service = Arc::new(
            BLSService::new("0x14d50ac943d01069c206543a0bed3836f6062b35270607ebf1d1f238ceda26f1")
                .unwrap(),
        );
        // Create mev-boost
        let mev_boost = MevBoost::new(" http://localhost:8080/eth/v1/builder/constraints", 123);
        // Some random constraints
        let constraint1 = generate_random_vec_with_random_size(50, 200);
        let constraint2 = generate_random_vec_with_random_size(50, 200);
        // Random slot_id
        let mut rng = rand::thread_rng();
        let slot_id = rng.gen_range(200..=5000) as u64;
        // call mev-boost
        mev_boost
            .force_inclusion(vec![constraint1, constraint2], slot_id, bls_service)
            .await
            .unwrap();

        // Check result
        // Retrieve logs from the container
        let logs = Command::new("docker")
            .args(&["logs", "mev-boost-mock"])
            .output()
            .expect("Failed to get container logs");

        let logs_str = String::from_utf8(logs.stdout).expect("Invalid UTF-8 sequence");

        assert!(
            !logs_str.contains("VerifySignature:  false"),
            "Some Signature is invalid"
        );
    }
}
