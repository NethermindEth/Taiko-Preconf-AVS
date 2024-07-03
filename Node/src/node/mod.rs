use crate::{ethereum_l1::EthereumL1, mev_boost::MevBoost, taiko::Taiko};
use anyhow::{anyhow as any_err, Error, Ok};
use tokio::sync::mpsc::{Receiver, Sender};

pub struct Node {
    taiko: Taiko,
    node_rx: Option<Receiver<String>>,
    avs_p2p_tx: Sender<String>,
    gas_used: u64,
    ethereum_l1: EthereumL1,
    mev_boost: MevBoost,
}

impl Node {
    pub fn new(
        node_rx: Receiver<String>,
        avs_p2p_tx: Sender<String>,
        taiko: Taiko,
        ethereum_l1: EthereumL1,
        mev_boost: MevBoost,
    ) -> Self {
        Self {
            taiko,
            node_rx: Some(node_rx),
            avs_p2p_tx,
            gas_used: 0,
            ethereum_l1,
            mev_boost,
        }
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn entrypoint(mut self) -> Result<(), Error> {
        tracing::info!("Starting node");
        self.start_new_msg_receiver_thread();
        self.preconfirmation_loop().await;
        Ok(())
    }

    fn start_new_msg_receiver_thread(&mut self) {
        if let Some(node_rx) = self.node_rx.take() {
            tokio::spawn(async move {
                Self::handle_incoming_messages(node_rx).await;
            });
        } else {
            tracing::error!("node_rx has already been moved");
        }
    }

    async fn handle_incoming_messages(mut node_rx: Receiver<String>) {
        loop {
            tokio::select! {
                Some(message) = node_rx.recv() => {
                    tracing::debug!("Node received message: {}", message);
                },
            }
        }
    }

    async fn preconfirmation_loop(&self) {
        loop {
            let start_time = tokio::time::Instant::now();
            if let Err(err) = self.main_block_preconfirmation_step().await {
                tracing::error!("Failed to execute main block preconfirmation step: {}", err);
            }
            let elapsed = start_time.elapsed();
            let sleep_duration = std::time::Duration::from_secs(4).saturating_sub(elapsed);
            tokio::time::sleep(sleep_duration).await;
        }
    }

    async fn main_block_preconfirmation_step(&self) -> Result<(), Error> {
        let pending_tx_lists = self
            .taiko
            .get_pending_l2_tx_lists()
            .await
            .map_err(Error::from)?;
        if pending_tx_lists.tx_list_bytes.len() == 0 {
            return Ok(());
        }

        self.commit_to_the_tx_lists();
        self.send_preconfirmations_to_the_avs_p2p().await?;
        self.taiko
            .advance_head_to_new_l2_block(pending_tx_lists.tx_lists, self.gas_used)
            .await?;
        let tx = self
            .ethereum_l1
            .create_propose_new_block_tx(
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
                    .parse()
                    .unwrap(),
                pending_tx_lists.tx_list_bytes[0].clone(), //TODO: handle rest tx lists
                pending_tx_lists.parent_meta_hash,
            )
            .await?;
        self.mev_boost.send_transaction(&tx, 1, 1);
        Ok(())
    }

    fn commit_to_the_tx_lists(&self) {
        //TODO: implement
    }

    async fn send_preconfirmations_to_the_avs_p2p(&self) -> Result<(), Error> {
        self.avs_p2p_tx
            .send("Hello from node!".to_string())
            .await
            .map_err(|e| any_err!("Failed to send message to avs_p2p_tx: {}", e))
    }
}
