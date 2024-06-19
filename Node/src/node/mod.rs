use crate::taiko::Taiko;
use tokio::sync::mpsc::{Receiver, Sender};

pub struct Node {
    taiko: Taiko,
    node_rx: Option<Receiver<String>>,
    avs_p2p_tx: Sender<String>,
}

impl Node {
    pub fn new(node_rx: Receiver<String>, avs_p2p_tx: Sender<String>) -> Self {
        let taiko = Taiko::new("http://127.0.0.1:1234");
        Self {
            taiko,
            node_rx: Some(node_rx),
            avs_p2p_tx,
        }
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn start(mut self) {
        tracing::info!("Starting node");
        self.start_new_msg_receiver_thread();
        self.main_block_preconfirmation_loop().await;
    }

    async fn main_block_preconfirmation_loop(&self) {
        loop {
            let _tx_lists = match self.taiko.get_pending_l2_tx_lists().await {
                Ok(lists) => lists,
                Err(err) => {
                    tracing::error!("Failed to get pending l2 tx lists: {}", err);
                    continue;
                }
            };
            self.commit_to_the_tx_lists();
            self.send_preconfirmations_to_the_avs_p2p();
            self.taiko.submit_new_l2_blocks();

            //TODO: remove after implementation of above methods
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    fn commit_to_the_tx_lists(&self) {
        //TODO: implement
    }

    fn send_preconfirmations_to_the_avs_p2p(&self) {
        let avs_p2p_tx = self.avs_p2p_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = avs_p2p_tx.send("Hello from node!".to_string()).await {
                tracing::error!("Failed to send message to avs_p2p_tx: {}", e);
            }
        });
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
                }
            }
        }
    }
}
