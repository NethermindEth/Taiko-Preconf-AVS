use crate::ethereum_l1::{execution_layer::PreconfTaskManager, validator::Validator, EthereumL1};
use anyhow::Error;
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error};

type LookaheadUpdated = Vec<PreconfTaskManager::LookaheadSetParam>;

#[derive(Clone)]
pub struct LookaheadUpdatedEventReceiver {
    ethereum_l1: Arc<EthereumL1>,
}

impl LookaheadUpdatedEventReceiver {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        Self { ethereum_l1 }
    }

    pub fn start(self) {
        tokio::spawn(async move {
            self.check_for_events().await;
        });
    }

    async fn check_for_events(self) {
        let event_poller = match self
            .ethereum_l1
            .execution_layer
            .subscribe_to_lookahead_updated_event()
            .await
        {
            Ok(event_stream) => event_stream,
            Err(e) => {
                error!("Error subscribing to lookahead updated event: {:?}", e);
                return;
            }
        };

        let mut stream = event_poller.0.into_stream();
        loop {
            match stream.next().await {
                Some(log) => match log {
                    Ok(log) => {
                        let lookahead_params = log.0._0;
                        debug!(
                            "Received lookahead updated event with {} params.",
                            lookahead_params.len()
                        );
                        if let Err(e) = self.check_lookahead_correctness(&lookahead_params).await {
                            error!("Error checking lookahead correctness: {:?}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving lookahead updated event: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                },
                None => {
                    error!("No lookahead updated event received, stream closed");
                    // TODO: recreate a stream in this case?
                }
            }
        }
    }

    async fn check_lookahead_correctness(
        &self,
        lookahead_updated_next_epoch: &LookaheadUpdated,
    ) -> Result<(), Error> {
        let epoch = self.ethereum_l1.slot_clock.get_current_epoch()?;
        let next_epoch_begin_timestamp = self
            .ethereum_l1
            .slot_clock
            .get_epoch_begin_timestamp(epoch + 1)?;

        let next_epoch_duties = self
            .ethereum_l1
            .consensus_layer
            .get_lookahead(epoch + 1)
            .await?;
        let next_epoch_lookahead_params = self
            .ethereum_l1
            .execution_layer
            .get_lookahead_params_for_epoch_using_cl_lookahead(
                next_epoch_begin_timestamp,
                &next_epoch_duties,
            )
            .await?;

        for (i, (param, updated_param)) in next_epoch_lookahead_params
            .iter()
            .zip(lookahead_updated_next_epoch.iter())
            .enumerate()
        {
            if param.timestamp != updated_param.timestamp
                || param.preconfer != updated_param.preconfer
            {
                error!("Mismatch found at index {i}");
                let pub_key = next_epoch_duties[i].public_key.clone();
                let slot = self
                    .ethereum_l1
                    .slot_clock
                    .slot_of(Duration::from_secs(param.timestamp.try_into()?))?;
                let validator = self
                    .ethereum_l1
                    .consensus_layer
                    .get_validator(pub_key, slot)
                    .await?;

                let validator = match Validator::try_from(validator) {
                    Ok(validator) => validator,
                    Err(e) => {
                        error!(
                            "Error converting validator to our validator struct: {:?}",
                            e
                        );
                        continue;
                    }
                };

                // pass validator to the prove method
            }
        }

        // self.ethereum_l1.execution_layer.prove
        Ok(())
    }
}
