use super::{
    avs_contract_error::AVSContractError,
    block_proposed::{BlockProposedV2, EventSubscriptionBlockProposedV2, TaikoEvents},
    slot_clock::SlotClock,
};
use crate::{
    bls::BLSService,
    ethereum_l1::ws_provider::WsProvider,
    utils::{config, types::*},
};
use alloy::{
    consensus::TypedTransaction,
    contract::EventSubscription,
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, Bytes, FixedBytes, B256, U256},
    providers::{Provider, ProviderBuilder, WsConnect},
    signers::{
        local::{LocalSigner, PrivateKeySigner},
        Signature, SignerSync,
    },
    sol,
    sol_types::SolValue,
};
use anyhow::Error;
use beacon_api_client::ProposerDuty;
use ecdsa::SigningKey;
use futures_util::StreamExt;
use k256::Secp256k1;
#[cfg(test)]
use mockall::automock;
use rand_core::{OsRng, RngCore};
use std::str::FromStr;
use std::sync::Arc;

pub struct ExecutionLayer {
    provider_ws: WsProvider,
    signer: LocalSigner<SigningKey<Secp256k1>>,
    wallet: EthereumWallet,
    preconfer_address: Address,
    contract_addresses: ContractAddresses,
    slot_clock: Arc<SlotClock>,
    msg_expiry_sec: u64,
    l1_chain_id: u64,
    bls_service: Arc<BLSService>,
}

pub struct ContractAddresses {
    pub taiko_l1: Address,
    pub eigen_layer: EigenLayerContractAddresses,
    pub avs: AvsContractAddresses,
}

pub struct EigenLayerContractAddresses {
    pub strategy_manager: Address,
    pub slasher: Address,
}

pub struct AvsContractAddresses {
    pub preconf_task_manager: Address,
    pub directory: Address,
    pub service_manager: Address,
    pub preconf_registry: Address,
}

sol!(
    #[allow(clippy::too_many_arguments)]
    #[allow(missing_docs)]
    #[sol(rpc)]
    PreconfTaskManager,
    "src/ethereum_l1/abi/PreconfTaskManager.json"
);

sol! {
    /// @dev Represents proposeBlock's _data input parameter
    struct BlockParamsV2 {
        address proposer;
        address coinbase;
        bytes32 parentMetaHash;
        uint64 anchorBlockId; // NEW
        uint64 timestamp; // NEW
        uint32 blobTxListOffset; // NEW
        uint32 blobTxListLength; // NEW
        uint8 blobIndex; // NEW
    }
}

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    StrategyManager,
    "src/ethereum_l1/abi/StrategyManager.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    Slasher,
    "src/ethereum_l1/abi/Slasher.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    AVSDirectory,
    "src/ethereum_l1/abi/AVSDirectory.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    PreconfRegistry,
    "src/ethereum_l1/abi/PreconfRegistry.json"
);

sol! (
    struct MessageData {
        uint256 chainId;
        uint8 op;
        uint256 expiry;
        address prefer;
    }
);

pub struct EventSubscriptionLookaheadUpdated(
    pub EventSubscription<PreconfTaskManager::LookaheadUpdated>,
);

#[cfg_attr(test, allow(dead_code))]
#[cfg_attr(test, automock)]
impl ExecutionLayer {
    pub async fn new(
        ws_rpc_url: &str,
        avs_node_ecdsa_private_key: &str,
        contract_addresses: &config::ContractAddresses,
        slot_clock: Arc<SlotClock>,
        msg_expiry_sec: u64,
        bls_service: Arc<BLSService>,
    ) -> Result<Self, Error> {
        tracing::debug!("Creating ExecutionLayer with WS URL: {}", ws_rpc_url);

        let signer = PrivateKeySigner::from_str(avs_node_ecdsa_private_key)?;
        let preconfer_address: Address = signer.address();
        tracing::info!("AVS node address: {}", preconfer_address);

        let wallet = EthereumWallet::from(signer.clone());

        let contract_addresses = Self::parse_contract_addresses(contract_addresses)
            .map_err(|e| Error::msg(format!("Failed to parse contract addresses: {}", e)))?;

        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: WsProvider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet.clone())
            .on_ws(ws.clone())
            .await
            .unwrap();

        let l1_chain_id = provider_ws.get_chain_id().await?;

        Ok(Self {
            provider_ws,
            signer,
            wallet,
            preconfer_address,
            contract_addresses,
            slot_clock,
            msg_expiry_sec,
            l1_chain_id,
            bls_service,
        })
    }

    pub fn get_preconfer_address(&self) -> PreconferAddress {
        self.preconfer_address.into_array()
    }

    fn parse_contract_addresses(
        contract_addresses: &config::ContractAddresses,
    ) -> Result<ContractAddresses, Error> {
        let eigen_layer = EigenLayerContractAddresses {
            strategy_manager: contract_addresses.eigen_layer.strategy_manager.parse()?,
            slasher: contract_addresses.eigen_layer.slasher.parse()?,
        };

        let avs = AvsContractAddresses {
            preconf_task_manager: contract_addresses.avs.preconf_task_manager.parse()?,
            directory: contract_addresses.avs.directory.parse()?,
            service_manager: contract_addresses.avs.service_manager.parse()?,
            preconf_registry: contract_addresses.avs.preconf_registry.parse()?,
        };

        let taiko_l1 = contract_addresses.taiko_l1.parse()?;

        Ok(ContractAddresses {
            taiko_l1,
            eigen_layer,
            avs,
        })
    }

    pub async fn propose_new_block(
        &self,
        nonce: u64,
        tx_list: Vec<u8>,
        lookahead_pointer: u64,
        lookahead_set_params: Vec<PreconfTaskManager::LookaheadSetParam>,
        send_to_contract: bool,
    ) -> Result<Vec<u8>, Error> {
        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let block_params = BlockParamsV2 {
            proposer: Address::ZERO,
            coinbase: <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(
                &self.wallet,
            ),
            parentMetaHash: FixedBytes::from(&[0u8; 32]),
            anchorBlockId: 0,
            timestamp: 0,
            blobTxListOffset: 0,
            blobTxListLength: 0,
            blobIndex: 0,
        };

        let encoded_block_params = Bytes::from(BlockParamsV2::abi_encode_sequence(&block_params));

        let tx_list = Bytes::from(tx_list);

        // TODO check gas parameters
        let builder = contract
            .newBlockProposal(
                vec![encoded_block_params],
                vec![tx_list],
                U256::from(lookahead_pointer),
                lookahead_set_params,
            )
            .chain_id(self.l1_chain_id)
            .nonce(nonce)
            .gas(1_000_000)
            .max_fee_per_gas(20_000_000_000)
            .max_priority_fee_per_gas(1_000_000_000);

        // Build transaction
        let tx = builder.as_ref().clone().build_typed_tx();
        let Ok(TypedTransaction::Eip1559(mut tx)) = tx else {
            return Err(anyhow::anyhow!(
                "propose_new_block: Not EIP1559 transaction"
            ));
        };

        // Sign transaction
        let signature = self
            .wallet
            .default_signer()
            .sign_transaction(&mut tx)
            .await?;

        // Encode transaction
        let mut buf = vec![];
        tx.encode_with_signature(&signature, &mut buf, false);

        // Send transaction
        if send_to_contract {
            let pending = self.provider_ws.send_raw_transaction(&buf).await?;

            tracing::debug!("Sending raw transaction, with hash {}", pending.tx_hash());
        }

        Ok(buf)
    }

    pub async fn register_preconfer(&self) -> Result<(), Error> {
        tracing::debug!("Registering preconfer");
        let strategy_manager = StrategyManager::new(
            self.contract_addresses.eigen_layer.strategy_manager,
            &self.provider_ws,
        );
        let one_eth = U256::from(1000000000000000000u64);
        match strategy_manager
            .depositIntoStrategy(Address::ZERO, Address::ZERO, one_eth)
            .value(one_eth)
            .send()
            .await
        {
            Ok(receipt) => {
                let tx_hash = receipt.watch().await?;
                tracing::info!("Deposited into strategy: {tx_hash}");
            }
            Err(err) => {
                tracing::error!("Depositing into strategy failed: {}", err);
            }
        }

        let slasher = Slasher::new(
            self.contract_addresses.eigen_layer.slasher,
            &self.provider_ws,
        );
        match slasher
            .optIntoSlashing(self.contract_addresses.avs.service_manager)
            .send()
            .await
        {
            Ok(receipt) => {
                let tx_hash = receipt.watch().await?;
                tracing::info!("Opted into slashing: {tx_hash}");
            }
            Err(err) => {
                tracing::error!("Opting into slashing failed: {}", err);
            }
        }

        let salt = Self::create_random_salt();
        let expiration_timestamp =
            U256::from(chrono::Utc::now().timestamp() as u64 + self.msg_expiry_sec);

        #[cfg(not(test))]
        let digest_hash_bytes = self
            .calculate_digest_hash(expiration_timestamp, salt)
            .await?;
        #[cfg(test)]
        let digest_hash_bytes = vec![0u8; 32]; // Dummy value for tests

        // sign the digest hash with private key
        let signature = self.signer.sign_message_sync(&digest_hash_bytes)?;

        let signature_with_salt_and_expiry = PreconfRegistry::SignatureWithSaltAndExpiry {
            signature: Bytes::from(signature.as_bytes()),
            salt,
            expiry: expiration_timestamp,
        };

        let preconf_registry = PreconfRegistry::new(
            self.contract_addresses.avs.preconf_registry,
            &self.provider_ws,
        );
        let tx = preconf_registry.registerPreconfer(signature_with_salt_and_expiry);

        match tx.send().await {
            Ok(pending_tx) => {
                let tx_hash = pending_tx.tx_hash();
                tracing::info!("Preconfer registered: {:?}", tx_hash);
            }
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "Preconfer registration failed: {}",
                    err.to_avs_contract_error()
                ));
            }
        }

        Ok(())
    }

    async fn calculate_digest_hash(
        &self,
        expiration_timestamp: U256,
        salt: FixedBytes<32>,
    ) -> Result<Vec<u8>, Error> {
        let avs_directory =
            AVSDirectory::new(self.contract_addresses.avs.directory, &self.provider_ws);

        let digest_hash = avs_directory
            .calculateOperatorAVSRegistrationDigestHash(
                self.preconfer_address,
                self.contract_addresses.avs.service_manager,
                salt,
                expiration_timestamp,
            )
            .call()
            .await?;

        Ok(digest_hash._0.to_vec())
    }

    fn create_random_salt() -> FixedBytes<32> {
        let mut salt: [u8; 32] = [0u8; 32];
        let mut os_rng = OsRng {};
        os_rng.fill_bytes(&mut salt);
        FixedBytes::from(&salt)
    }

    pub fn sign_message_with_private_ecdsa_key(&self, msg: &[u8]) -> Result<[u8; 65], Error> {
        let signature = self.signer.sign_message_sync(msg)?;
        Ok(signature.as_bytes())
    }

    pub fn recover_address_from_msg(&self, msg: &[u8], signature: &[u8]) -> Result<Address, Error> {
        let signature = Signature::try_from(signature)?;
        let address = signature.recover_address_from_msg(msg)?;
        Ok(address)
    }

    pub async fn get_preconfer_nonce(&self) -> Result<u64, Error> {
        let nonce = self
            .provider_ws
            .get_transaction_count(self.preconfer_address)
            .await?;
        Ok(nonce)
    }

    pub async fn check_and_prove_incorrect_preconfirmation(
        &self,
        chain_id: u64,
        preconf_tx_list_hash: [u8; 32],
        preconf_signature: [u8; 65],
        block_proposed: &BlockProposedV2,
    ) -> Result<(), Error> {
        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let header = PreconfTaskManager::PreconfirmationHeader {
            blockId: block_proposed.event_data().blockId,
            chainId: U256::from(chain_id),
            txListHash: B256::from(preconf_tx_list_hash),
        };
        let signature = Bytes::from(preconf_signature);

        let proposed_meta = &block_proposed.event_data().meta;
        let meta = PreconfTaskManager::BlockMetadataV2 {
            anchorBlockHash: proposed_meta.anchorBlockHash,
            difficulty: proposed_meta.difficulty,
            blobHash: proposed_meta.blobHash,
            extraData: proposed_meta.extraData,
            coinbase: proposed_meta.coinbase,
            id: proposed_meta.id,
            gasLimit: proposed_meta.gasLimit,
            timestamp: proposed_meta.timestamp,
            anchorBlockId: proposed_meta.anchorBlockId,
            baseFeeConfig: PreconfTaskManager::BaseFeeConfig {
                adjustmentQuotient: proposed_meta.baseFeeConfig.adjustmentQuotient,
                sharingPctg: proposed_meta.baseFeeConfig.sharingPctg,
                gasIssuancePerSecond: proposed_meta.baseFeeConfig.gasIssuancePerSecond,
                minGasExcess: proposed_meta.baseFeeConfig.minGasExcess,
                maxGasIssuancePerBlock: proposed_meta.baseFeeConfig.maxGasIssuancePerBlock,
            },
            parentMetaHash: proposed_meta.parentMetaHash,
            blobUsed: proposed_meta.blobUsed,
            blobTxListOffset: proposed_meta.blobTxListOffset,
            blobTxListLength: proposed_meta.blobTxListLength,
            blobIndex: proposed_meta.blobIndex,
            livenessBond: proposed_meta.livenessBond,
            proposedAt: proposed_meta.proposedAt,
            proposedIn: proposed_meta.proposedIn,
            minTier: proposed_meta.minTier,
            proposer: proposed_meta.proposer,
        };
        let result = contract
            .proveIncorrectPreconfirmation(meta.clone(), header.clone(), signature.clone())
            .call()
            .await;
        if result.is_ok() {
            tracing::debug!("Proved incorrect preconfirmation using eth_call, sending tx");
            let tx = contract.proveIncorrectPreconfirmation(meta, header, signature);
            match tx.send().await {
                Ok(pending_tx) => {
                    let tx_hash = pending_tx.tx_hash();
                    tracing::debug!("Proved incorrect preconfirmation, tx sent: {tx_hash}");
                }
                Err(err) => {
                    tracing::error!("Failed to prove incorrect preconfirmation: {}", err);
                }
            }
        } else {
            tracing::debug!(
                "Preconfirmation correct for the block {}",
                block_proposed.block_id()
            );
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn prove_incorrect_lookahead(
        &self,
        lookahead_pointer: u64,
        slot_timestamp: u64,
        validator_bls_pub_key: BLSCompressedPublicKey,
        validator: &[u8],
        validator_index: usize,
        validator_proof: Vec<[u8; 32]>,
        validators_root: [u8; 32],
        beacon_state_proof: Vec<[u8; 32]>,
        beacon_state_root: [u8; 32],
        beacon_block_proof_for_state: Vec<[u8; 32]>,
        beacon_block_proof_for_proposer_index: Vec<[u8; 32]>,
    ) -> Result<(), Error> {
        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let mut validator_chunks: [B256; 8] = Default::default();
        for (i, chunk) in validator.chunks(32).enumerate() {
            validator_chunks[i] = B256::from_slice(chunk);
        }
        let validator_index = U256::from(validator_index);

        let validator_inclusion_proof = PreconfTaskManager::InclusionProof {
            validator: validator_chunks,
            validatorIndex: validator_index,
            validatorProof: Self::convert_proof_to_fixed_bytes(validator_proof),
            validatorsRoot: FixedBytes::from(validators_root),
            beaconStateProof: Self::convert_proof_to_fixed_bytes(beacon_state_proof),
            beaconStateRoot: FixedBytes::from(beacon_state_root),
            beaconBlockProofForState: Self::convert_proof_to_fixed_bytes(
                beacon_block_proof_for_state,
            ),
            beaconBlockProofForProposerIndex: Self::convert_proof_to_fixed_bytes(
                beacon_block_proof_for_proposer_index,
            ),
        };
        let tx = contract.proveIncorrectLookahead(
            U256::from(lookahead_pointer),
            U256::from(slot_timestamp),
            Bytes::from(validator_bls_pub_key),
            validator_inclusion_proof,
        );
        match tx.send().await {
            Ok(pending_tx) => {
                let tx_hash = pending_tx.tx_hash();
                tracing::debug!("Proved incorrect lookahead: {tx_hash}");
            }
            Err(err) => {
                tracing::error!("Failed to prove incorrect lookahead: {}", err);
            }
        }

        Ok(())
    }

    fn convert_proof_to_fixed_bytes(proof: Vec<[u8; 32]>) -> Vec<FixedBytes<32>> {
        proof.iter().map(FixedBytes::from).collect()
    }

    pub async fn subscribe_to_registered_event(
        &self,
    ) -> Result<EventSubscription<PreconfRegistry::PreconferRegistered>, Error> {
        let registry = PreconfRegistry::new(
            self.contract_addresses.avs.preconf_registry,
            &self.provider_ws,
        );

        let registered_filter = registry.PreconferRegistered_filter().subscribe().await?;
        tracing::debug!("Subscribed to registered event");

        Ok(registered_filter)
    }

    pub async fn wait_for_the_registered_event(
        &self,
        registered_filter: EventSubscription<PreconfRegistry::PreconferRegistered>,
    ) -> Result<(), Error> {
        let mut stream = registered_filter.into_stream();
        while let Some(log) = stream.next().await {
            match log {
                Ok(log) => {
                    tracing::info!("Received PreconferRegistered for: {}", log.0.preconfer);
                    if log.0.preconfer == self.preconfer_address {
                        tracing::info!("Preconfer registered!");
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Error receiving log: {:?}", e);
                }
            }
        }

        Ok(())
    }

    pub async fn force_push_lookahead(
        &self,
        lookahead_set_params: Vec<PreconfTaskManager::LookaheadSetParam>,
    ) -> Result<(), Error> {
        tracing::debug!(
            "Force pushing lookahead, {} params",
            lookahead_set_params.len()
        );

        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let tx = contract
            .forcePushLookahead(lookahead_set_params)
            .nonce(self.get_preconfer_nonce().await?)
            .gas(10_000_000)
            .max_fee_per_gas(20_000_000_000)
            .max_priority_fee_per_gas(1_000_000_000);

        match tx.send().await {
            Ok(receipt) => {
                tracing::debug!("Force push lookahead sent: {}", receipt.tx_hash());
            }
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "force_push_lookahead: {}",
                    err.to_avs_contract_error()
                ));
            }
        }

        Ok(())
    }

    pub async fn add_validator(&self) -> Result<(), Error> {
        // Build add message
        // Operation.ADD
        let operation = 1;
        // Message expired after 60 seconds
        let expiry = U256::from(chrono::Utc::now().timestamp() as u64 + self.msg_expiry_sec);

        let data = MessageData::from((
            U256::from(self.l1_chain_id),
            operation,
            expiry,
            self.preconfer_address,
        ));
        let message = data.abi_encode_packed();

        // Convert bls public key to G1Point
        let pk_point = self.bls_service.pubkey_to_g1_point();
        let pubkey = PreconfRegistry::G1Point {
            x: pk_point[0],
            y: pk_point[1],
        };

        let signature = self.bls_service.sign(&message, &[]);
        // Sign message and convert to G2Point
        let signature_point = self.bls_service.signature_to_g2_point(&signature);

        let signature = PreconfRegistry::G2Point {
            x: signature_point[0],
            x_I: signature_point[1],
            y: signature_point[2],
            y_I: signature_point[3],
        };

        // Call contract
        let params = vec![PreconfRegistry::AddValidatorParam {
            pubkey,
            signature,
            signatureExpiry: expiry,
        }];

        let preconf_registry = PreconfRegistry::new(
            self.contract_addresses.avs.preconf_registry,
            &self.provider_ws,
        );
        let tx = preconf_registry.addValidators(params);

        match tx.send().await {
            Ok(pending_tx) => {
                let tx_hash = pending_tx.tx_hash();
                tracing::info!("Add validator to preconfer successful: {:?}", tx_hash);
            }
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "add_validator: {}",
                    err.to_avs_contract_error()
                ));
            }
        }

        Ok(())
    }

    pub async fn remove_validator(&self) -> Result<(), Error> {
        // Build remove message
        // Operation.REMOVE
        let operation = 2;
        // Message expired after 60 seconds
        let expiry = U256::from(chrono::Utc::now().timestamp() as u64 + self.msg_expiry_sec);

        let data = MessageData::from((
            U256::from(self.l1_chain_id),
            operation,
            expiry,
            self.preconfer_address,
        ));
        let message = data.abi_encode_packed();

        // Convert bls public key to G1Point
        let pk_point = self.bls_service.pubkey_to_g1_point();
        let pubkey = PreconfRegistry::G1Point {
            x: pk_point[0],
            y: pk_point[1],
        };

        let signature = self.bls_service.sign(&message, &[]);
        // Sign message and convert to G2Point
        let signature_point = self.bls_service.signature_to_g2_point(&signature);

        let signature = PreconfRegistry::G2Point {
            x: signature_point[0],
            x_I: signature_point[1],
            y: signature_point[2],
            y_I: signature_point[3],
        };

        // Call contract
        let params = vec![PreconfRegistry::RemoveValidatorParam {
            pubkey,
            signature,
            signatureExpiry: expiry,
        }];

        let preconf_registry = PreconfRegistry::new(
            self.contract_addresses.avs.preconf_registry,
            &self.provider_ws,
        );
        let tx = preconf_registry.removeValidators(params);

        match tx.send().await {
            Ok(pending_tx) => {
                let tx_hash = pending_tx.tx_hash();
                tracing::info!("Validator removed successfully: {:?}", tx_hash);
            }
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "remove_validator: {}",
                    err.to_avs_contract_error()
                ));
            }
        }

        Ok(())
    }

    pub async fn subscribe_to_validator_added_event(
        &self,
    ) -> Result<EventSubscription<PreconfRegistry::ValidatorAdded>, Error> {
        let registry = PreconfRegistry::new(
            self.contract_addresses.avs.preconf_registry,
            &self.provider_ws,
        );

        let validator_added_filter = registry.ValidatorAdded_filter().subscribe().await?;
        tracing::debug!("Subscribed to ValidatorAdded event");

        Ok(validator_added_filter)
    }

    pub async fn wait_for_the_validator_added_event(
        &self,
        validator_added_filter: EventSubscription<PreconfRegistry::ValidatorAdded>,
    ) -> Result<(), Error> {
        let mut stream = validator_added_filter.into_stream();
        while let Some(log) = stream.next().await {
            match log {
                Ok(log) => {
                    tracing::info!(
                        "Received ValidatorAdded for:\npubkey hash: {}\npreconfer: {}",
                        log.0.pubKeyHash,
                        log.0.preconfer
                    );
                    if log.0.preconfer == self.preconfer_address {
                        tracing::info!("Validator added!");
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Error receiving log: {}", e);
                }
            }
        }
        Ok(())
    }

    pub async fn subscribe_to_lookahead_updated_event(
        &self,
    ) -> Result<EventSubscriptionLookaheadUpdated, Error> {
        let task_manager = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let lookahead_updated_filter = task_manager.LookaheadUpdated_filter().subscribe().await?;
        tracing::debug!("Subscribed to lookahead updated event");

        Ok(EventSubscriptionLookaheadUpdated(lookahead_updated_filter))
    }

    pub async fn subscribe_to_block_proposed_event(
        &self,
    ) -> Result<EventSubscriptionBlockProposedV2, Error> {
        let taiko_events = TaikoEvents::new(self.contract_addresses.taiko_l1, &self.provider_ws);

        let block_proposed_filter = taiko_events.BlockProposedV2_filter().subscribe().await?;
        tracing::debug!("Subscribed to block proposed V2 event");

        Ok(EventSubscriptionBlockProposedV2(block_proposed_filter))
    }

    pub async fn get_lookahead_params_for_epoch_using_cl_lookahead(
        &self,
        epoch: u64,
        cl_lookahead: &[ProposerDuty],
    ) -> Result<Vec<PreconfTaskManager::LookaheadSetParam>, Error> {
        let epoch_begin_timestamp = self
            .slot_clock
            .get_real_epoch_begin_timestamp_for_contract(epoch)?;
        tracing::debug!(
            "Epoch {}, timestamp: {}, getting lookahead params for epoch using CL lookahead len: {}",
            epoch,
            epoch_begin_timestamp,
            cl_lookahead.len()
        );

        if cl_lookahead.len() != self.slot_clock.get_slots_per_epoch() as usize {
            return Err(anyhow::anyhow!(
                "get_lookahead_params_for_epoch_using_cl_lookahead: unexpected number of proposer duties in the lookahead"
            ));
        }

        let slots = self.slot_clock.get_slots_per_epoch() as usize;
        let validator_bls_pub_keys: Vec<BLSCompressedPublicKey> = cl_lookahead
            .iter()
            .take(slots)
            .map(|key| {
                let mut array = [0u8; 48];
                array.copy_from_slice(&key.public_key);
                array
            })
            .collect();

        self.get_lookahead_params_for_epoch(
            epoch_begin_timestamp,
            validator_bls_pub_keys.as_slice().try_into()?,
        )
        .await
    }

    async fn get_lookahead_params_for_epoch(
        &self,
        epoch_begin_timestamp: u64,
        validator_bls_pub_keys: &[BLSCompressedPublicKey; 32],
    ) -> Result<Vec<PreconfTaskManager::LookaheadSetParam>, Error> {
        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let params = match contract
            .getLookaheadParamsForEpoch(
                U256::from(epoch_begin_timestamp),
                validator_bls_pub_keys.map(Bytes::from),
            )
            .call()
            .await
        {
            Ok(params) => params._0,
            Err(err) => {
                let raw_result = contract
                    .getLookaheadParamsForEpoch(
                        U256::from(epoch_begin_timestamp),
                        validator_bls_pub_keys.map(Bytes::from),
                    )
                    .call_raw()
                    .await;
                self.check_raw_result(raw_result);

                return Err(anyhow::anyhow!(
                    "get_lookahead_params_for_epoch: {}",
                    err.to_avs_contract_error()
                ));
            }
        };

        Ok(params)
    }

    fn check_raw_result(&self, raw_result: Result<Bytes, alloy::contract::Error>) {
        tracing::debug!("Raw result: {:?}", raw_result);
        if let Ok(raw_result) = raw_result {
            if raw_result.is_empty() {
                tracing::error!("Raw result is empty, contract {} does not have any code, check the contract address and RPC URL", self.contract_addresses.avs.preconf_task_manager);
            }
        }
    }

    pub async fn get_lookahead_preconfer_addresses_for_epoch(
        &self,
        epoch: u64,
    ) -> Result<Vec<PreconferAddress>, Error> {
        tracing::debug!("Getting lookahead preconfer addresses for epoch: {}", epoch);
        let epoch_begin_timestamp = self
            .slot_clock
            .get_real_epoch_begin_timestamp_for_contract(epoch)?;

        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let lookahead = contract
            .getLookaheadForEpoch(U256::from(epoch_begin_timestamp))
            .call()
            .await?
            ._0;

        tracing::debug!(
            "getLookaheadForEpoch({}) result: {:?}",
            epoch_begin_timestamp,
            lookahead
        );
        Ok(lookahead
            .iter()
            .map(|addr| addr.into_array())
            .collect::<Vec<PreconferAddress>>())
    }

    pub async fn get_lookahead_preconfer_buffer(
        &self,
    ) -> Result<[PreconfTaskManager::LookaheadBufferEntry; 128], Error> {
        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let lookahead = contract.getLookaheadBuffer().call().await?._0;

        Ok(lookahead)
    }

    pub async fn is_lookahead_required(&self) -> Result<bool, Error> {
        let contract = PreconfTaskManager::new(
            self.contract_addresses.avs.preconf_task_manager,
            &self.provider_ws,
        );

        let is_required = contract.isLookaheadRequired().call().await;

        match is_required {
            Ok(is_required) => {
                tracing::debug!("is_lookahead_required for next epoch: {}", is_required._0);
                Ok(is_required._0)
            }
            Err(err) => {
                let raw_result = contract.isLookaheadRequired().call_raw().await;
                self.check_raw_result(raw_result);

                Err(anyhow::anyhow!(
                    "is_lookahead_required: {}",
                    err.to_avs_contract_error()
                ))
            }
        }
    }

    #[cfg(test)]
    pub async fn new_from_pk(
        ws_rpc_url: String,
        rpc_url: reqwest::Url,
        private_key: elliptic_curve::SecretKey<k256::Secp256k1>,
    ) -> Result<Self, Error> {
        let signer = PrivateKeySigner::from_signing_key(private_key.into());
        let wallet = EthereumWallet::from(signer.clone());
        let clock = SlotClock::new(0u64, 12u64, 12u64, 32u64, 3u64);

        let provider = ProviderBuilder::new().on_http(rpc_url.clone());
        let l1_chain_id = provider.get_chain_id().await?;

        let bls_service = Arc::new(
            crate::bls::BLSService::new(
                "0x14d50ac943d01069c206543a0bed3836f6062b35270607ebf1d1f238ceda26f1",
            )
            .unwrap(),
        );

        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: WsProvider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet.clone())
            .on_ws(ws.clone())
            .await
            .unwrap();

        Ok(Self {
            provider_ws,
            signer,
            wallet,
            preconfer_address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // some random address for test
                .parse()?,
            slot_clock: Arc::new(clock),
            contract_addresses: ContractAddresses {
                taiko_l1: Address::ZERO,
                eigen_layer: EigenLayerContractAddresses {
                    strategy_manager: Address::ZERO,
                    slasher: Address::ZERO,
                },
                avs: AvsContractAddresses {
                    preconf_task_manager: Address::ZERO,
                    directory: Address::ZERO,
                    service_manager: Address::ZERO,
                    preconf_registry: Address::ZERO,
                },
            },
            msg_expiry_sec: 120,
            bls_service,
            l1_chain_id,
        })
    }

    #[cfg(test)]
    async fn call_test_contract(&self) -> Result<(), Error> {
        sol! {
            #[allow(missing_docs)]
            #[sol(rpc, bytecode="6080806040523460135760df908160198239f35b600080fdfe6080806040526004361015601257600080fd5b60003560e01c9081633fb5c1cb1460925781638381f58a146079575063d09de08a14603c57600080fd5b3460745760003660031901126074576000546000198114605e57600101600055005b634e487b7160e01b600052601160045260246000fd5b600080fd5b3460745760003660031901126074576020906000548152f35b34607457602036600319011260745760043560005500fea2646970667358221220e978270883b7baed10810c4079c941512e93a7ba1cd1108c781d4bc738d9090564736f6c634300081a0033")]
            contract Counter {
                uint256 public number;

                function setNumber(uint256 newNumber) public {
                    number = newNumber;
                }

                function increment() public {
                    number++;
                }
            }
        }

        let contract = Counter::deploy(&self.provider_ws).await?;

        let builder = contract.setNumber(U256::from(42));
        let tx_hash = builder.send().await?.watch().await?;
        println!("Set number to 42: {tx_hash}");

        let builder = contract.increment();
        let tx_hash = builder.send().await?.watch().await?;
        println!("Incremented number: {tx_hash}");

        let builder = contract.number();
        let number = builder.call().await?.number.to_string();

        assert_eq!(number, "43");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::node_bindings::Anvil;

    #[tokio::test]
    async fn test_call_contract() {
        // Ensure `anvil` is available in $PATH.
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(ws_rpc_url, rpc_url, private_key)
            .await
            .unwrap();
        el.call_test_contract().await.unwrap();
    }

    #[tokio::test]
    async fn test_propose_new_block() {
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(ws_rpc_url, rpc_url, private_key)
            .await
            .unwrap();

        el.propose_new_block(0, vec![0; 32], 0, vec![], true)
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn test_register() {
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(ws_rpc_url, rpc_url, private_key)
            .await
            .unwrap();

        let result = el.register_preconfer().await;
        assert!(result.is_ok(), "Register method failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_get_lookahead_params_for_epoch() {
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let _el = ExecutionLayer::new_from_pk(ws_rpc_url, rpc_url, private_key)
            .await
            .unwrap();

        let _epoch_begin_timestamp = 0;
        let _validator_bls_pub_keys: [BLSCompressedPublicKey; 32] = [[0u8; 48]; 32];

        // TODO:
        // There is a bug in the Anvil (anvil 0.2.0) library:
        // `Result::unwrap()` on an `Err` value: buffer overrun while deserializing
        // check if it's fixed in next version
        // let lookahead_params = el
        //     .get_lookahead_params_for_epoch(epoch_begin_timestamp, &validator_bls_pub_keys)
        //     .await
        //     .unwrap();
        // assert!(
        //     !lookahead_params.is_empty(),
        //     "Lookahead params should not be empty"
        // );
    }

    #[tokio::test]
    async fn test_prove_incorrect_lookahead() {
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(ws_rpc_url, rpc_url, private_key)
            .await
            .unwrap();

        // Test parameters
        let lookahead_pointer = 100;
        let slot_timestamp = 1000;
        let validator_bls_pub_key = [1u8; 48];
        let validator = vec![2u8; 256];
        let validator_index = 0;
        let validator_proof = vec![[3u8; 32]; 5];
        let validators_root = [4u8; 32];
        let beacon_state_proof = vec![[5u8; 32]; 5];
        let beacon_state_root = [6u8; 32];
        let beacon_block_proof_for_state = vec![[7u8; 32]; 5];
        let beacon_block_proof_for_proposer_index = vec![[8u8; 32]; 5];

        // Call the method
        let result = el
            .prove_incorrect_lookahead(
                lookahead_pointer,
                slot_timestamp,
                validator_bls_pub_key,
                &validator,
                validator_index,
                validator_proof,
                validators_root,
                beacon_state_proof,
                beacon_state_root,
                beacon_block_proof_for_state,
                beacon_block_proof_for_proposer_index,
            )
            .await;

        // Assert the result
        assert!(result.is_ok(), "prove_incorrect_lookahead should succeed");
    }
}
