use alloy::contract::Error;
use alloy::transports::TransportError;

pub trait AVSContractError {
    fn to_avs_contract_error(&self) -> String;
}

impl AVSContractError for Error {
    fn to_avs_contract_error(&self) -> String {
        if let Error::TransportError(TransportError::ErrorResp(e)) = self {
            if let Some(data) = e.data.as_ref().map(|d| d.get()) {
                if let Ok(error_code) = u32::from_str_radix(&data[3..data.len() - 1], 16) {
                    return match error_code {
                        // PreconfRegistry
                        0xc3be614d => "PreconferAlreadyRegistered(): The preconfer is already registered in the registry".to_string(),
                        0x3b6c5f24 => "PreconferNotRegistered(): The preconfer is not registered in the registry".to_string(),
                        0x62278171 => "InvalidValidatorSignature(): The signature is invalid".to_string(),
                        0x6c06063b => "ValidatorSignatureExpired(): The signature has expired".to_string(),
                        0x43f8f5e9 => "ValidatorAlreadyActive(): The validator is already proposing for a preconfer and cannot be added again without removal".to_string(),
                        0x2868d153 => "ValidatorAlreadyInactive(): The validator is already removed or waiting to stop proposing for a preconfer".to_string(),
                        // PreconfServiceManager
                        0x42faedfe => "SenderIsNotPreconfTaskManager(): Only callable by the task manager".to_string(),
                        0xcbb45a59 => "SenderIsNotPreconfRegistry(): Only callable by the registry".to_string(),
                        0x54357847 => "OperatorAlreadySlashed(): The operator is already slashed".to_string(),
                        // PreconfTaskManager
                        0xbe4e4f53 => "InvalidLookaheadPointer(): The current (or provided) timestamp does not fall in the range provided by the lookahead pointer".to_string(),
                        0x1502053a => "SenderIsNotThePreconfer(): The block proposer is not the assigned preconfer for the current slot/timestamp".to_string(),
                        //0x3b6c5f24 => "PreconferNotRegistered(): Preconfer is not present in the registry".to_string(),
                        0x971cce8f => "InvalidSlotTimestamp(): The timestamp in the lookahead is not of a valid future slot in the present epoch".to_string(),
                        0xd74dc18f => "PreconfirmationChainIdMismatch(): The chain id on which the preconfirmation was signed is different from the current chain's id".to_string(),
                        0x005b3ac2 => "MissedDisputeWindow(): The dispute window for proving incorrect lookahead or preconfirmation is over".to_string(),
                        0x7855bfd4 => "PreconfirmationIsCorrect(): The disputed preconfirmation is correct".to_string(),
                        0xc0abeb9f => "MetadataMismatch(): The sent block metadata does not match the one retrieved from Taiko".to_string(),
                        0xf23592f9 => "PosterAlreadySlashedOrLookaheadIsEmpty(): The lookahead poster for the epoch has already been slashed or there is no lookahead for epoch".to_string(),
                        0xdd52015e => "LookaheadEntryIsCorrect(): The lookahead preconfer matches the one the actual validator is proposing for".to_string(),
                        0xc08f610e => "LookaheadIsNotRequired(): Cannot force push a lookahead since it is not lagging behind".to_string(),
                        // Taiko L1
                        0x3a0e4c1a => "L1_FORK_ERROR(): TaikoL1 error.".to_string(),
                        0x36c7c689 => "L1_INVALID_PARAMS(): TaikoL1 error.".to_string(),
                        0xdf9969ef => "L1_BLOB_NOT_AVAILABLE(): TaikoL1 error.".to_string(),
                        0x9e7e2ddd => "L1_BLOB_NOT_FOUND(): TaikoL1 error.".to_string(),
                        0x618e4902 => "L1_INVALID_ANCHOR_BLOCK(): TaikoL1 error.".to_string(),
                        0xc043062a => "L1_INVALID_CUSTOM_PROPOSER(): TaikoL1 error.".to_string(),
                        0xd6e2c5a0 => "L1_INVALID_PROPOSER(): TaikoL1 error.".to_string(),
                        0x13f7f80d => "L1_INVALID_TIMESTAMP(): TaikoL1 error.".to_string(),
                        0x37e6fc42 => "L1_LIVENESS_BOND_NOT_RECEIVED(): TaikoL1 error.".to_string(),
                        0x51ec7d53 => "L1_TOO_MANY_BLOCKS(): TaikoL1 error.".to_string(),
                        0x1a83d90e => "L1_UNEXPECTED_PARENT(): TaikoL1 error.".to_string(),
                        _ => format!("Unknown error {:#?}", self),
                    };
                }
            }
        }
        format!("Unknown error {:#?}", self)
    }
}

//tests

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{
        network::EthereumWallet, node_bindings::Anvil, providers::ProviderBuilder,
        signers::local::PrivateKeySigner, sol,
    };

    // Generate a contract instance from Solidity.
    sol!(
        #[allow(missing_docs)]
        #[sol(rpc, bytecode = "6080604052348015600f57600080fd5b5060ff8061001e6000396000f3fe6080604052348015600f57600080fd5b506004361060465760003560e01c80631e453c0c14604b5780637303c6a61460535780638ee77a16146059578063bb2152c214605f575b600080fd5b60516065565b005b6051607e565b60516097565b605160b0565b604051639e7e2ddd60e01b815260040160405180910390fd5b604051630edb17c960e21b815260040160405180910390fd5b604051635435784760e01b815260040160405180910390fd5b604051630a81029d60e11b815260040160405180910390fdfea26469706673582212206f805f93cf208ffab73436edb23ab3ed3095a3e8de4e45118b4f430518b18b3d64736f6c63430008190033")]
        contract ErrorContract {

            error PreconferNotRegistered();
            error OperatorAlreadySlashed();
            error SenderIsNotThePreconfer();
            error L1_BLOB_NOT_FOUND();


            function errorPreconferNotRegistered() public {
                revert PreconferNotRegistered();
            }

            function errorOperatorAlreadySlashed() public {
                revert OperatorAlreadySlashed();
            }

            function errorSenderIsNotThePreconfer() public {
                revert SenderIsNotThePreconfer();
            }

            function errorL1BlobNotFound() public {
                revert L1_BLOB_NOT_FOUND();
            }

        }
    );

    #[tokio::test]
    async fn test_error_decoding() {
        let anvil = Anvil::new().try_spawn().unwrap();

        // Create a provider.
        let rpc_url: alloy::transports::http::reqwest::Url = anvil.endpoint().parse().unwrap();
        let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
        signer.address();
        let wallet = EthereumWallet::from(signer);

        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet.clone())
            .on_http(rpc_url.clone());

        // Deploy the `Counter` contract.
        let contract = ErrorContract::deploy(&provider).await.unwrap();

        let result = contract.errorPreconferNotRegistered().call().await;
        if let Err(e) = result {
            assert_eq!(
                e.to_avs_contract_error(),
                "PreconferNotRegistered(): The preconfer is not registered in the registry"
            );
        } else {
            assert!(false);
        }

        let result = contract.errorOperatorAlreadySlashed().call().await;
        if let Err(e) = result {
            assert_eq!(
                e.to_avs_contract_error(),
                "OperatorAlreadySlashed(): The operator is already slashed"
            );
        } else {
            assert!(false);
        }

        let result = contract.errorSenderIsNotThePreconfer().call().await;
        if let Err(e) = result {
            assert_eq!(
                e.to_avs_contract_error(),
                "SenderIsNotThePreconfer(): The block proposer is not the assigned preconfer for the current slot/timestamp"
            );
        } else {
            assert!(false);
        }

        let result = contract.errorL1BlobNotFound().call().await;
        if let Err(e) = result {
            assert_eq!(
                e.to_avs_contract_error(),
                "L1_BLOB_NOT_FOUND(): TaikoL1 error."
            );
        } else {
            assert!(false);
        }
    }
}
