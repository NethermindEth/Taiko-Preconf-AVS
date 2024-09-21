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
                        _ => format!("Unknown error {:#?}", self),
                    };
                }
            }
        }
        format!("Unknown error {:#?}", self)
    }
}
