use crate::{
    ethereum_l1::{
        execution_layer::PreconfTaskManager, ssz_serializer::*, validator::Validator, EthereumL1,
    },
    utils::types::*,
};
use anyhow::Error;
use beacon_api_client::ProposerDuty;
use futures_util::StreamExt;
use rs_merkle::{algorithms::Sha256, Hasher, MerkleTree};
use ssz::Encode;
use std::sync::Arc;
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
        let next_epoch = epoch + 1;
        let next_epoch_begin_timestamp = self
            .ethereum_l1
            .slot_clock
            .get_epoch_begin_timestamp(next_epoch)?;

        let next_epoch_duties = self
            .ethereum_l1
            .consensus_layer
            .get_lookahead(next_epoch)
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
                info!("Lookahead mismatch found at index {i}");
                let pub_key = next_epoch_duties[i].public_key.as_ref();
                let validator_index = next_epoch_duties[i].validator_index;
                let validators = self
                    .ethereum_l1
                    .consensus_layer
                    .get_all_validators_for_head_state()
                    .await?;

                let leaves_index = validators
                    .iter()
                    .position(|v| v.public_key == pub_key)
                    .ok_or(anyhow::anyhow!(
                        "Validator not found in the all validators list from the beacon chain"
                    ))?;
                let validator = &validators[leaves_index];

                let (validator_proof, validators_root) =
                    Self::create_merkle_proof_for_validator_being_part_of_validator_list(
                        &validators,
                        leaves_index,
                    )?;

                let (beacon_state_proof, beacon_state_root) = self
                    .create_merkle_proof_for_validator_list_being_part_of_beacon_state()
                    .await?;

                self.ethereum_l1
                    .execution_layer
                    .prove_incorrect_lookahead(
                        0,
                        0,
                        0,
                        validator,
                        validator_index,
                        &validator_proof,
                        validators_root,
                        &beacon_state_proof,
                        beacon_state_root,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    fn create_merkle_proof_for_validator_being_part_of_validator_list(
        validators: &[Validator],
        leaves_index: usize,
    ) -> Result<(Vec<u8>, [u8; 32]), Error> {
        let ssz_encoded_validators = validators
            .iter()
            .map(|v| v.as_ssz_bytes())
            .collect::<Vec<_>>();
        let leaves: Vec<[u8; 32]> = ssz_encoded_validators
            .iter()
            .map(|v| Sha256::hash(v))
            .collect();

        let merkle_tree = MerkleTree::<Sha256>::from_leaves(&leaves);
        let indices_to_prove = vec![leaves_index];
        let merkle_proof = merkle_tree.proof(&indices_to_prove);
        let proof_bytes = merkle_proof.to_bytes();
        let root = merkle_tree
            .root()
            .ok_or(anyhow::anyhow!("couldn't get the merkle root"))?;
        Ok((proof_bytes, root))
    }

    async fn create_merkle_proof_for_validator_list_being_part_of_beacon_state(
        &self,
    ) -> Result<(Vec<u8>, [u8; 32]), Error> {
        const VALIDATORS_INDEX: usize = 11;
        let beacon_state = self.ethereum_l1.consensus_layer.get_beacon_state().await?;
        let ssz_encoded_fields =
            serialize_beacon_state_fields_to_vec_of_ssz_encoded_bytes(&beacon_state)?;
        let leaves: Vec<[u8; 32]> = ssz_encoded_fields.iter().map(|v| Sha256::hash(v)).collect();

        let merkle_tree = MerkleTree::<Sha256>::from_leaves(&leaves);
        let indices_to_prove = vec![VALIDATORS_INDEX];
        let merkle_proof = merkle_tree.proof(&indices_to_prove);
        let proof_bytes = merkle_proof.to_bytes();
        let root = merkle_tree
            .root()
            .ok_or(anyhow::anyhow!("couldn't get the merkle root"))?;
        Ok((proof_bytes, root))
    }
}
