use super::block_proposed::{EventSubscriptionBlockProposedV2, TaikoEvents};
use crate::{
    ethereum_l1::ws_provider::WsProvider,
    utils::{config, types::*},
};
use alloy::{
    consensus::{transaction::SignableTransaction, TypedTransaction},
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, Bytes, FixedBytes},
    providers::{Provider, ProviderBuilder, WsConnect},
    signers::{
        local::{LocalSigner, PrivateKeySigner},
        SignerSync,
    },
    sol,
    sol_types::SolValue,
};
use anyhow::Error;
use ecdsa::SigningKey;
use k256::Secp256k1;
#[cfg(test)]
use mockall::automock;
use std::str::FromStr;

pub struct ExecutionLayer {
    provider_ws: WsProvider,
    signer: LocalSigner<SigningKey<Secp256k1>>,
    wallet: EthereumWallet,
    preconfer_address: Address,
    contract_addresses: ContractAddresses,
    l1_chain_id: u64,
}

pub struct ContractAddresses {
    pub taiko_l1: Address,
    pub preconf_whitelist: Address,
    pub preconf_router: Address,
    pub avs: AvsContractAddresses,
}

pub struct AvsContractAddresses {
    pub preconf_task_manager: Address,
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

sol! {
    // https://github.com/NethermindEth/preconf-taiko-mono/blob/main/packages/protocol/contracts/layer1/based/ITaikoInbox.sol
    struct BlockParams {
        // the max number of transactions in this block. Note that if there are not enough
        // transactions in calldata or blobs, the block will contains as many transactions as
        // possible.
        uint16 numTransactions;
        // For the first block in a batch,  the block timestamp is the batch params' `timestamp`
        // plus this time shift value;
        // For all other blocks in the same batch, the block timestamp is its parent block's
        // timestamp plus this time shift value.
        uint8 timeShift;
        // Signals sent on L1 and need to sync to this L2 block.
        bytes32[] signalSlots;
    }

    struct BlobParams {
        // The hashes of the blob. Note that if this array is not empty.  `firstBlobIndex` and
        // `numBlobs` must be 0.
        bytes32[] blobHashes;
        // The index of the first blob in this batch.
        uint8 firstBlobIndex;
        // The number of blobs in this batch. Blobs are initially concatenated and subsequently
        // decompressed via Zlib.
        uint8 numBlobs;
        // The byte offset of the blob in the batch.
        uint32 byteOffset;
        // The byte size of the blob.
        uint32 byteSize;
        // The block number when the blob was created.
        uint64 createdIn;
    }

    struct BatchParams {
        address proposer;
        address coinbase;
        bytes32 parentMetaHash;
        uint64 anchorBlockId;
        uint64 lastBlockTimestamp;
        bool revertIfNotFirstProposal;
        // Specifies the number of blocks to be generated from this batch.
        BlobParams blobParams;
        BlockParams[] blocks;
    }

    struct ProposeBatchWrapper {
        bytes bytesX;
        bytes bytesY;
    }
}

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    PreconfRouter,
    "src/ethereum_l1/abi/PreconfRouter.json"
);

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
    PreconfWhitelist,
    "src/ethereum_l1/abi/PreconfWhitelist.json"
);

sol! (
    struct MessageData {
        uint256 chainId;
        uint8 op;
        uint256 expiry;
        address prefer;
    }
);

#[cfg_attr(test, allow(dead_code))]
#[cfg_attr(test, automock)]
impl ExecutionLayer {
    pub async fn new(
        ws_rpc_url: &str,
        avs_node_ecdsa_private_key: &str,
        contract_addresses: &config::ContractAddresses,
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
            l1_chain_id,
        })
    }

    pub fn get_preconfer_address(&self) -> PreconferAddress {
        self.preconfer_address.into_array()
    }

    fn parse_contract_addresses(
        contract_addresses: &config::ContractAddresses,
    ) -> Result<ContractAddresses, Error> {
        let avs = AvsContractAddresses {
            preconf_task_manager: contract_addresses.avs.preconf_task_manager.parse()?,
        };

        let taiko_l1 = contract_addresses.taiko_l1.parse()?;
        let preconf_whitelist = contract_addresses.preconf_whitelist.parse()?;
        let preconf_router = contract_addresses.preconf_router.parse()?;

        Ok(ContractAddresses {
            taiko_l1,
            preconf_whitelist,
            preconf_router,
            avs,
        })
    }

    pub async fn get_operator_for_current_epoch(&self) -> Result<Address, Error> {
        let contract =
            PreconfWhitelist::new(self.contract_addresses.preconf_whitelist, &self.provider_ws);
        let operator = contract.getOperatorForCurrentEpoch().call().await?._0;
        Ok(operator)
    }

    pub async fn get_operator_for_next_epoch(&self) -> Result<Address, Error> {
        let contract =
            PreconfWhitelist::new(self.contract_addresses.preconf_whitelist, &self.provider_ws);
        let operator = contract.getOperatorForNextEpoch().call().await?._0;
        Ok(operator)
    }

    pub async fn propose_batch(
        &self,
        nonce: u64,
        tx_list: Vec<u8>,
        tx_count: u16,
    ) -> Result<Vec<u8>, Error> {
        let contract =
            PreconfRouter::new(self.contract_addresses.preconf_router, &self.provider_ws);

        let tx_list_len = tx_list.len() as u32;
        let tx_list = Bytes::from(tx_list);

        let bytes_x = Bytes::new();

        let block_params = BlockParams {
            numTransactions: tx_count,
            timeShift: 0,
            signalSlots: vec!(),
        };

        let batch_params = BatchParams {
            proposer: self.preconfer_address.clone(),
            coinbase: <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(
                &self.wallet,
            ),
            parentMetaHash: FixedBytes::from(&[0u8; 32]),
            anchorBlockId: 0,
            lastBlockTimestamp: 0,
            revertIfNotFirstProposal: false,
            blobParams: BlobParams {
                blobHashes: vec![],
                firstBlobIndex: 0,
                numBlobs: 0,
                byteOffset: 0,
                byteSize: tx_list_len,
                createdIn: 0,
            },
            blocks: vec![block_params],
        };

        let encoded_batch_params = Bytes::from(BatchParams::abi_encode(&batch_params));

        let propose_batch_wrapper = ProposeBatchWrapper {
            bytesX: bytes_x,
            bytesY: encoded_batch_params,
        };

        let encoded_propose_batch_wrapper = Bytes::from(ProposeBatchWrapper::abi_encode_sequence(
            &propose_batch_wrapper,
        ));
        // TODO check gas parameters
        let builder = contract
            .proposeBatch(encoded_propose_batch_wrapper, tx_list)
            .chain_id(self.l1_chain_id)
            .nonce(nonce)
            .gas(1_000_000)
            .max_fee_per_gas(20_000_000_000)
            .max_priority_fee_per_gas(1_000_000_000);

        // Build transaction
        let tx = builder.into_transaction_request().build_typed_tx();
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

        let mut encoded = Vec::new();
        tx.into_signed(signature).rlp_encode(&mut encoded);
        // add EIP-1559 type
        encoded.insert(0, 0x02);

        // Send transaction
        let pending = self.provider_ws.send_raw_transaction(&encoded).await?;
        tracing::debug!("Sending raw transaction, with hash {}", pending.tx_hash());

        Ok(encoded)
    }

    pub fn sign_message_with_private_ecdsa_key(&self, msg: &[u8]) -> Result<[u8; 65], Error> {
        let signature = self.signer.sign_message_sync(msg)?;
        Ok(signature.as_bytes())
    }

    pub async fn get_preconfer_nonce(&self) -> Result<u64, Error> {
        let nonce = self
            .provider_ws
            .get_transaction_count(self.preconfer_address)
            .await?;
        Ok(nonce)
    }

    pub async fn subscribe_to_block_proposed_event(
        &self,
    ) -> Result<EventSubscriptionBlockProposedV2, Error> {
        let taiko_events = TaikoEvents::new(self.contract_addresses.taiko_l1, &self.provider_ws);

        let block_proposed_filter = taiko_events.BlockProposedV2_filter().subscribe().await?;
        tracing::debug!("Subscribed to block proposed V2 event");

        Ok(EventSubscriptionBlockProposedV2(block_proposed_filter))
    }

    #[cfg(test)]
    pub async fn new_from_pk(
        ws_rpc_url: String,
        rpc_url: reqwest::Url,
        private_key: elliptic_curve::SecretKey<k256::Secp256k1>,
    ) -> Result<Self, Error> {
        let signer = PrivateKeySigner::from_signing_key(private_key.into());
        let wallet = EthereumWallet::from(signer.clone());

        let provider = ProviderBuilder::new().on_http(rpc_url.clone());
        let l1_chain_id = provider.get_chain_id().await?;

        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: WsProvider = ProviderBuilder::new()
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
            contract_addresses: ContractAddresses {
                taiko_l1: Address::ZERO,
                preconf_whitelist: Address::ZERO,
                preconf_router: Address::ZERO,
                avs: AvsContractAddresses {
                    preconf_task_manager: Address::ZERO,
                },
            },
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

        let builder = contract.setNumber(alloy::primitives::U256::from(42));
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
}
