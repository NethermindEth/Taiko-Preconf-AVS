use crate::taiko::Taiko;
use anyhow::{anyhow as err, Context, Error};
use tokio::sync::mpsc::{Receiver, Sender};

pub struct Node {
    taiko: Taiko,
    node_rx: Receiver<String>,
    avs_p2p_tx: Sender<String>,
    gas_used: u64,
}

impl Node {
    pub fn new(node_rx: Receiver<String>, avs_p2p_tx: Sender<String>) -> Self {
        let taiko = Taiko::new("http://127.0.0.1:1234", "http://127.0.0.1:1235");
        Self {
            taiko,
            node_rx,
            avs_p2p_tx,
            gas_used: 0,
        }
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn entrypoint(mut self) -> Result<(), Error> {
        tracing::info!("Starting node");
        loop {
            if let Err(err) = self.step().await {
                tracing::error!("Node processing step failed: {}", err);
            }
        }
    }

    async fn step(&mut self) -> Result<(), Error> {
        if let Ok(msg) = self.node_rx.try_recv() {
            self.process_incoming_message(msg).await?;
        } else {
            self.main_block_preconfirmation_step().await?;
        }
        Ok(())
    }

    async fn main_block_preconfirmation_step(&self) -> Result<(), Error> {
        let pending_tx_lists = self
            .taiko
            .get_pending_l2_tx_lists()
            .await
            .map_err(Error::from)?;
        self.commit_to_the_tx_lists();
        self.send_preconfirmations_to_the_avs_p2p().await?;
        self.taiko
            .advance_head_to_new_l2_block(pending_tx_lists, self.gas_used)
            .await?;
        Ok(())
    }

    async fn process_incoming_message(&mut self, msg: String) -> Result<(), Error> {
        tracing::debug!("Node received message: {}", msg);
        Ok(())
    }

    fn commit_to_the_tx_lists(&self) {
        //TODO: implement
    }

    async fn send_preconfirmations_to_the_avs_p2p(&self) -> Result<(), Error> {
        self.avs_p2p_tx
            .send("Hello from node!".to_string())
            .await
            .map_err(|e| err!("Failed to send message to avs_p2p_tx: {}", e))
    }
}
