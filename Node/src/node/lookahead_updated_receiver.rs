use crate::{
    ethereum_l1::{execution_layer::PreconfTaskManager, merkle_proofs::*, EthereumL1},
    utils::types::*,
};
use anyhow::Error;
use beacon_api_client::ProposerDuty;
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, info};

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
                        let handler = LookaheadUpdatedEventHandler::new(self.ethereum_l1.clone());
                        handler.handle_lookahead_updated_event(lookahead_params);
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
}

pub struct LookaheadUpdatedEventHandler {
    ethereum_l1: Arc<EthereumL1>,
}

impl LookaheadUpdatedEventHandler {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        Self { ethereum_l1 }
    }

    pub fn handle_lookahead_updated_event(
        self,
        lookahead_params: Vec<PreconfTaskManager::LookaheadSetParam>,
    ) {
        tokio::spawn(async move {
            if let Err(e) = self.check_lookahead_correctness(lookahead_params).await {
                error!("Error checking lookahead correctness: {:?}", e);
            }
        });
    }

    async fn check_lookahead_correctness(
        &self,
        lookahead_updated_next_epoch: LookaheadUpdated,
    ) -> Result<(), Error> {
        let epoch = self
            .ethereum_l1
            .slot_clock
            .get_epoch_for_timestamp(lookahead_updated_next_epoch[0].timestamp.try_into()?)?;

        let epoch_begin_timestamp = self
            .ethereum_l1
            .slot_clock
            .get_epoch_begin_timestamp(epoch)?;
        let epoch_duties = self
            .ethereum_l1
            .consensus_layer
            .get_lookahead(epoch)
            .await?;
        let epoch_lookahead_params = self
            .ethereum_l1
            .execution_layer
            .get_lookahead_params_for_epoch_using_cl_lookahead(epoch_begin_timestamp, &epoch_duties)
            .await?;

        if let Some(slot_timestamp) = Self::find_a_slot_timestamp_to_prove_incorrect_lookahead(
            &epoch_lookahead_params,
            &lookahead_updated_next_epoch,
        )? {
            let slot = self
                .ethereum_l1
                .slot_clock
                .slot_of(Duration::from_secs(slot_timestamp))?;
            let corresponding_epoch_slot_index =
                (slot % self.ethereum_l1.slot_clock.get_slots_per_epoch()) as usize;
            self.wait_for_the_slot_to_prove_incorrect_lookahead(slot + 1)
                .await?;
            self.prove_incorrect_lookahead(
                slot,
                slot_timestamp,
                &epoch_duties[corresponding_epoch_slot_index],
            )
            .await?;
        }

        Ok(())
    }

    fn find_a_slot_timestamp_to_prove_incorrect_lookahead(
        lookahead_params: &[PreconfTaskManager::LookaheadSetParam],
        lookahead_updated_event_params: &[PreconfTaskManager::LookaheadSetParam],
    ) -> Result<Option<u64>, Error> {
        // compare corresponding params in the two lists
        for (param, updated_param) in lookahead_params
            .iter()
            .zip(lookahead_updated_event_params.iter())
        {
            if param.preconfer != updated_param.preconfer
                || param.timestamp != updated_param.timestamp
            {
                return Ok(Some(updated_param.timestamp.try_into()?));
            }
        }

        if lookahead_params.len() > lookahead_updated_event_params.len() {
            // the lookahead updated doesn't contain enough params
            let first_proper_lookahead_params_missing_in_the_event =
                &lookahead_params[lookahead_updated_event_params.len()];
            return Ok(Some(
                first_proper_lookahead_params_missing_in_the_event
                    .timestamp
                    .try_into()?,
            ));
        } else if lookahead_params.len() < lookahead_updated_event_params.len() {
            // the lookahead updated contains additional, wrong params
            let first_additional_wrong_param =
                &lookahead_updated_event_params[lookahead_params.len()];
            return Ok(Some(first_additional_wrong_param.timestamp.try_into()?));
        }

        return Ok(None);
    }

    async fn wait_for_the_slot_to_prove_incorrect_lookahead(
        &self,
        slot: Slot,
    ) -> Result<(), Error> {
        tokio::time::sleep(
            self.ethereum_l1
                .slot_clock
                .duration_to_slot_from_now(slot)?,
        )
        .await;
        Ok(())
    }

    async fn prove_incorrect_lookahead(
        &self,
        slot: Slot,
        slot_timestamp: u64,
        epoch_duty: &ProposerDuty,
    ) -> Result<(), Error> {
        info!("Lookahead mismatch found for slot: {}", slot);

        let next_slot = slot + 1;

        let lookahead_pointer = self.find_lookahead_pointer(slot_timestamp).await?;

        let pub_key = &epoch_duty.public_key;
        let beacon_state = self
            .ethereum_l1
            .consensus_layer
            .get_beacon_state(next_slot)
            .await?;
        let validators = beacon_state.validators();
        let validator_index = validators
            .iter()
            .position(|v| v.public_key == *pub_key)
            .ok_or(anyhow::anyhow!(
                "Validator not found in the all validators list from the beacon chain"
            ))?;
        let ssz_encoded_validator =
            serialize_validator_to_ssz_encoded_bytes(&validators[validator_index])?;

        let (validator_proof, validators_root) =
            create_merkle_proof_for_validator_being_part_of_validator_list(
                &validators,
                validator_index,
            )?;

        let (beacon_state_proof, beacon_state_root) =
            create_merkle_proof_for_validator_list_being_part_of_beacon_state(&beacon_state)?;

        let beacon_block = self
            .ethereum_l1
            .consensus_layer
            .get_beacon_block(next_slot)
            .await?;
        let (beacon_block_proof_for_state, beacon_block_proof_for_proposer_index) =
            create_merkle_proofs_for_beacon_block_containing_beacon_state_and_validator_index(
                &beacon_block,
            )?;

        self.ethereum_l1
            .execution_layer
            .prove_incorrect_lookahead(
                lookahead_pointer,
                slot_timestamp,
                pub_key.as_ref().try_into()?,
                &ssz_encoded_validator,
                validator_index,
                validator_proof,
                validators_root,
                beacon_state_proof,
                beacon_state_root,
                beacon_block_proof_for_state,
                beacon_block_proof_for_proposer_index,
            )
            .await
    }

    async fn find_lookahead_pointer(&self, slot_timestamp: u64) -> Result<u64, Error> {
        let lookahead_preconfer_buffer = self
            .ethereum_l1
            .execution_layer
            .get_lookahead_preconfer_buffer()
            .await?;

        lookahead_preconfer_buffer
            .iter()
            .position(|entry| {
                slot_timestamp > entry.prevTimestamp && slot_timestamp <= entry.timestamp
            })
            .ok_or(anyhow::anyhow!(
                "find_lookahead_pointer: Lookahead pointer not found"
            ))
            .map(|i| i as u64)
    }
}
