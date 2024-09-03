use anyhow::Error;
use ethereum_consensus::deneb::Validator;
use ethereum_consensus::types::{
    mainnet::{BeaconState, SignedBeaconBlock},
    BeaconBlockBodyRef, ExecutionPayloadHeaderRef,
};
use ssz_rs::prelude::*;

pub fn serialize_beacon_state_fields_to_vec_of_ssz_encoded_bytes(
    beacon_state: &BeaconState,
) -> Result<Vec<Vec<u8>>, Error> {
    let mut result = vec![];

    result.push(serialize(&beacon_state.genesis_time())?);
    result.push(serialize(&beacon_state.genesis_validators_root())?);
    result.push(serialize(&beacon_state.slot())?);
    result.push(serialize(beacon_state.fork())?);
    result.push(serialize(beacon_state.latest_block_header())?);
    result.push(serialize(beacon_state.block_roots())?);
    result.push(serialize(beacon_state.state_roots())?);
    result.push(serialize(beacon_state.historical_roots())?);
    result.push(serialize(beacon_state.eth1_data())?);
    result.push(serialize(beacon_state.eth1_data_votes())?);
    result.push(serialize(&beacon_state.eth1_deposit_index())?);
    result.push(serialize(beacon_state.validators())?);
    result.push(serialize(beacon_state.balances())?);
    result.push(serialize(beacon_state.randao_mixes())?);
    result.push(serialize(beacon_state.slashings())?);
    if let Some(participation) = beacon_state.previous_epoch_participation() {
        result.push(serialize(participation)?);
    }
    if let Some(participation) = beacon_state.current_epoch_participation() {
        result.push(serialize(participation)?);
    }
    result.push(serialize(beacon_state.justification_bits())?);
    result.push(serialize(beacon_state.previous_justified_checkpoint())?);
    result.push(serialize(beacon_state.current_justified_checkpoint())?);
    result.push(serialize(beacon_state.finalized_checkpoint())?);
    if let Some(inactivity_scores) = beacon_state.inactivity_scores() {
        result.push(serialize(inactivity_scores)?);
    }
    if let Some(sync_committee) = beacon_state.current_sync_committee() {
        result.push(serialize(sync_committee)?);
    }
    if let Some(sync_committee) = beacon_state.next_sync_committee() {
        result.push(serialize(sync_committee)?);
    }
    if let Some(execution_payload_header) = beacon_state.latest_execution_payload_header() {
        match execution_payload_header {
            ExecutionPayloadHeaderRef::Capella(execution_payload_header) => {
                result.push(serialize(execution_payload_header)?);
            }
            ExecutionPayloadHeaderRef::Deneb(execution_payload_header) => {
                result.push(serialize(execution_payload_header)?);
            }
            ExecutionPayloadHeaderRef::Bellatrix(execution_payload_header) => {
                result.push(serialize(execution_payload_header)?);
            }
        }
    }
    result.push(serialize(&beacon_state.next_withdrawal_index())?);
    result.push(serialize(&beacon_state.next_withdrawal_validator_index())?);
    if let Some(historical_summaries) = beacon_state.historical_summaries() {
        result.push(serialize(historical_summaries)?);
    }

    Ok(result)
}

pub fn serialize_beacon_block_fields_to_vec_of_ssz_encoded_bytes(
    beacon_block: &SignedBeaconBlock,
) -> Result<Vec<Vec<u8>>, Error> {
    let mut result = vec![];

    result.push(serialize(&beacon_block.message().slot())?);
    result.push(serialize(&beacon_block.message().proposer_index())?);
    result.push(serialize(&beacon_block.message().parent_root())?);
    result.push(serialize(&beacon_block.message().state_root())?);
    match beacon_block.message().body() {
        BeaconBlockBodyRef::Capella(body) => result.push(serialize(body)?),
        BeaconBlockBodyRef::Deneb(body) => result.push(serialize(body)?),
        BeaconBlockBodyRef::Bellatrix(body) => result.push(serialize(body)?),
        BeaconBlockBodyRef::Altair(body) => result.push(serialize(body)?),
        BeaconBlockBodyRef::Phase0(body) => result.push(serialize(body)?),
    }

    Ok(result)
}

pub fn serialize_validator_to_ssz_encoded_bytes(validator: &Validator) -> Result<Vec<u8>, Error> {
    serialize(validator).map_err(|e| anyhow::anyhow!("Failed to serialize validator: {e}"))
}
