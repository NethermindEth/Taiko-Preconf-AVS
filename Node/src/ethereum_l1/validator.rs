use std::convert::TryFrom;
use std::io::Read;

use crate::utils::types::*;
use ethereum_consensus::phase0::validator::Validator as EthereumConsensusValidator;
use ssz::{Decode, Encode};
use ssz_derive::{Decode, Encode};

#[derive(PartialEq, Debug, Encode, Decode)]
pub struct Validator {
    pub public_key: BLSCompressedPublicKey,
    pub withdrawal_credentials: [u8; 32],
    pub effective_balance: Gwei,
    pub slashed: bool,
    pub activation_eligibility_epoch: Epoch,
    pub activation_epoch: Epoch,
    pub exit_epoch: Epoch,
    pub withdrawable_epoch: Epoch,
}

impl TryFrom<EthereumConsensusValidator> for Validator {
    type Error = Box<dyn std::error::Error>;

    fn try_from(eth_validator: EthereumConsensusValidator) -> Result<Self, Self::Error> {
        Ok(Validator {
            public_key: eth_validator.public_key.as_ref().try_into()?,
            withdrawal_credentials: eth_validator.withdrawal_credentials.as_ref().try_into()?,
            effective_balance: eth_validator.effective_balance,
            slashed: eth_validator.slashed,
            activation_eligibility_epoch: eth_validator.activation_eligibility_epoch,
            activation_epoch: eth_validator.activation_epoch,
            exit_epoch: eth_validator.exit_epoch,
            withdrawable_epoch: eth_validator.withdrawable_epoch,
        })
    }
}
