use anyhow::Error;
use ethereum_consensus::deneb::Validator;
use ethereum_consensus::types::mainnet::{BeaconState, SignedBeaconBlock};
use ssz_rs::prelude::*;

pub fn create_merkle_proof_for_validator_being_part_of_validator_list<const N: usize>(
    validators: &List<Validator, N>,
    validator_index: usize,
) -> Result<(Vec<[u8; 32]>, [u8; 32]), Error> {
    let (_leaf, branch, witness, _generalized_index) =
        create_merkle_proof_for_validator_being_part_of_validator_list_extended(
            validators,
            validator_index,
        )?;

    Ok((branch, witness))
}

fn create_merkle_proof_for_validator_being_part_of_validator_list_extended<const N: usize>(
    validators: &List<Validator, N>,
    validator_index: usize,
) -> Result<([u8; 32], Vec<[u8; 32]>, [u8; 32], usize), Error> {
    let path = &[validator_index.into()];
    let (proof, witness) = validators
        .prove(path)
        .map_err(|e| anyhow::anyhow!("Failed to prove validator: {e}"))?;

    let leaf = proof.leaf.into();
    let branch = proof.branch.iter().map(|b| b.0.into()).collect();
    let witness = witness.into();
    Ok((leaf, branch, witness, proof.index))
}

pub fn create_merkle_proof_for_validator_list_being_part_of_beacon_state(
    beacon_state: &BeaconState,
) -> Result<(Vec<[u8; 32]>, [u8; 32]), Error> {
    let path = &["validators".into()];
    let (proof, witness) = match beacon_state {
        BeaconState::Deneb(state) => state
            .prove(path)
            .map_err(|e| anyhow::anyhow!("Failed to prove validator: {e}"))?,
        _ => return Err(anyhow::anyhow!("BeaconState is not in Deneb")),
    };

    let branch = proof.branch.iter().map(|b| b.0.into()).collect();
    let witness = witness.into();
    Ok((branch, witness))
}

pub fn create_merkle_proofs_for_beacon_block_containing_beacon_state_and_validator_index(
    beacon_block: &SignedBeaconBlock,
) -> Result<(Vec<[u8; 32]>, Vec<[u8; 32]>), Error> {
    let (state_root_prove, validator_index_prove) = match beacon_block {
        SignedBeaconBlock::Deneb(block) => {
            let path = &["state_root".into()];
            let (state_root_prove, _) = block.message.prove(path).map_err(|e| {
                anyhow::anyhow!("Failed to prove beacon state being part of beacon block: {e}")
            })?;

            let path = &["proposer_index".into()];
            let (validator_index_prove, _) = block.message.prove(path).map_err(|e| {
                anyhow::anyhow!("Failed to prove validator index being part of beacon block: {e}")
            })?;

            (state_root_prove, validator_index_prove)
        }
        _ => return Err(anyhow::anyhow!("BeaconBlock is not in Deneb")),
    };

    let state_root_branch = state_root_prove.branch.iter().map(|b| b.0.into()).collect();
    let validator_index_branch = validator_index_prove
        .branch
        .iter()
        .map(|b| b.0.into())
        .collect();

    Ok((state_root_branch, validator_index_branch))
}

pub fn serialize_validator_to_ssz_encoded_bytes(validator: &Validator) -> Result<Vec<u8>, Error> {
    validator
        .chunks()
        .map_err(|e| anyhow::anyhow!("Failed to read chunks for validator: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethereum_l1::merkle_proofs::tests::deneb::presets::mainnet::{
        BeaconBlock, BeaconBlockBody,
    };
    use crate::ethereum_l1::merkle_proofs::tests::deneb::BlsPublicKey;
    use crate::ethereum_l1::merkle_proofs::tests::deneb::Bytes32;
    use alloy::primitives::FixedBytes;
    use ethereum_consensus::{deneb, primitives::BlsSignature, types::mainnet::SignedBeaconBlock};
    use ssz_rs::proofs::Proof;
    use ssz_rs::List;

    #[test]
    fn test_create_merkle_proof_for_validator_being_part_of_validator_list() {
        let validators = List::<_, 2>::try_from(create_validators()).unwrap();
        let validator_index = 1;

        let (leaf, proof_branch, root_witness, generalized_index) =
            create_merkle_proof_for_validator_being_part_of_validator_list_extended(
                &validators,
                validator_index,
            )
            .expect("Proof generation should succeed");

        // Verify the proof
        let proof = Proof {
            leaf: FixedBytes::from(leaf),
            branch: proof_branch.iter().map(|b| FixedBytes::from(b)).collect(),
            index: generalized_index,
        };
        println!("proof: {:#?}", proof);

        let witness = root_witness.into();

        let result = proof.verify(witness);
        assert!(result.is_ok(), "Proof verification should succeed");
    }

    fn create_validators() -> Vec<Validator> {
        let validator = Validator {
            public_key: BlsPublicKey::try_from([0u8; 48].as_slice()).unwrap(),
            withdrawal_credentials: Bytes32::try_from([0u8; 32].as_slice()).unwrap(),
            effective_balance: 0,
            slashed: false,
            activation_eligibility_epoch: 0,
            activation_epoch: 0,
            exit_epoch: 0,
            withdrawable_epoch: 0,
        };

        let validator2 = Validator {
            public_key: BlsPublicKey::try_from(
                [
                    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                    23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42,
                    43, 44, 45, 46, 47, 48,
                ]
                .as_slice(),
            )
            .unwrap(),
            withdrawal_credentials: Bytes32::try_from([0u8; 32].as_slice()).unwrap(),
            effective_balance: 0,
            slashed: false,
            activation_eligibility_epoch: 0,
            activation_epoch: 0,
            exit_epoch: 0,
            withdrawable_epoch: 0,
        };

        vec![validator, validator2]
    }

    #[test]
    fn test_beacon_state_prove_validators_list() {
        let validators = create_validators();

        let beacon_state = BeaconState::Deneb(deneb::BeaconState {
            validators: List::try_from(vec![validators[0].clone()]).unwrap(),
            ..Default::default()
        });

        let beacon_state2 = BeaconState::Deneb(deneb::BeaconState {
            validators: List::try_from(vec![validators[1].clone()]).unwrap(),
            ..Default::default()
        });

        let path = &["validators".into()];

        match (beacon_state, beacon_state2) {
            (BeaconState::Deneb(state1), BeaconState::Deneb(state2)) => {
                let (proof, witness) = state1.prove(path).expect("Proof generation should succeed");
                assert_eq!(witness, state1.hash_tree_root().unwrap());
                assert!(proof.verify(witness).is_ok());

                let branch = proof.branch;
                dbg!(&branch);

                let (proof2, _witness2) =
                    state2.prove(path).expect("Proof generation should succeed");
                let result = proof2.verify(witness); // wrong witness used, shouldn't pass
                assert!(result.is_err());
            }

            _ => panic!("BeaconState is not in Deneb"),
        }
    }

    #[test]
    fn test_create_merkle_proofs_for_beacon_block_containing_beacon_state_and_validator_index() {
        let validators = create_validators();

        let beacon_state = BeaconState::Deneb(deneb::BeaconState {
            validators: List::try_from(vec![validators[0].clone()]).unwrap(),
            ..Default::default()
        });

        let beacon_block_body = BeaconBlockBody {
            // Populate the fields of the BeaconBlockBody as needed
            ..Default::default()
        };

        let beacon_block = SignedBeaconBlock::Deneb(deneb::SignedBeaconBlock {
            message: BeaconBlock {
                slot: 0,
                proposer_index: 0,
                parent_root: FixedBytes::from([0; 32]),
                state_root: beacon_state.hash_tree_root().unwrap(),
                body: beacon_block_body,
            },
            signature: BlsSignature::default(),
        });

        create_merkle_proofs_for_beacon_block_containing_beacon_state_and_validator_index(
            &beacon_block,
        )
        .expect("Proof generation should succeed");
    }

    #[test]
    fn test_serialize_validator_to_ssz_encoded_bytes() {
        let validator = create_validators()[0].clone();
        let serialized = serialize_validator_to_ssz_encoded_bytes(&validator).unwrap();
        assert_eq!(
            serialized.len(),
            256,
            "Serialized validator should be 256 bytes"
        );
    }

    #[test]
    fn test_validator_list_beacon_state_use_hex_values() {
        let leaf = FixedBytes::<32>::try_from(
            hex::decode("0ccf56d8e76d16306c6e6e78ec20c07be5fa5ae89b18873b43cc823075a5df0b")
                .unwrap()
                .as_slice(),
        )
        .unwrap();

        let branch = vec![
            FixedBytes::<32>::try_from(
                hex::decode("8c53160000000000000000000000000000000000000000000000000000000000")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
            FixedBytes::<32>::try_from(
                hex::decode("d9cb62ffd113d2a2b71b4539c54bf01587d8a2a5a7c81baa2c2ae89d245578d6")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
            FixedBytes::<32>::try_from(
                hex::decode("efbad4c97640101fc18122e8b818e8cc3c278a18e05dc601af4095d5519d834a")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
            FixedBytes::<32>::try_from(
                hex::decode("775d61d75ab0731115447847764383a42283b502eb4ed3ca7ba412ac67da5138")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
            FixedBytes::<32>::try_from(
                hex::decode("bb5cf5c0273b8d100f329ea0c78c471d0833f048c7fc264c285c3696d7aed412")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        ];

        // Verify the proof
        let proof = Proof {
            leaf,
            branch,
            index: 43,
        };

        let root_witness = FixedBytes::<32>::try_from(
            hex::decode("cd918afbe365c6dcabab551e32fae5f3f9677433876049dc035e5135122a2e7e")
                .unwrap()
                .as_slice(),
        )
        .unwrap();

        let state_root = root_witness.into();
        dbg!(&proof);
        dbg!(&state_root);

        assert!(
            proof.verify(state_root).is_ok(),
            "Proof verification should succeed"
        );
    }
}
