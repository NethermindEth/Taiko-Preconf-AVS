# Taiko Preconf Smart Contracts

These smart contracts constitute an **actively validated service (AVS)** that provides based preconfirmations of transactions for Taiko L2. The contracts are resposible for:

- Managing registration of staked preconfers who may deliver the preconfirmations on an external P2P layer.
- Slashing the stake of a preconfer for not respecting a preconfirmation i.e
  - They fail to propose the associated preconfirmed block in time, or
  - They manipulate the ordering of transactions in the proposed block.
- Managing an L1 validator-lookahead that enables the selection of the based-preconfer for the upcoming L1 epoch.

## Core contracts

![Architecture](https://github.com/user-attachments/assets/b4686edb-8fec-4b0f-b91e-222bfa1fe7f4)

> [!IMPORTANT]
> EIP-2537 that enables the precompiles for the BLS12381 curve is yet to go live on Ethereum mainnet. Therefore, the registry contract is only functional on a devnet with the pectra upgrade enabled.

> [!NOTE]
> BLS signature checks have been commented out for this POC.

### PreconfRegistry

- Handles registration of preconfers and their associated L1 validators.
- Preconfers need to prove the ownership of an L1 validator by signing a message using the validator's BLS key.

### PreconfTaskManager

- Routes L2 blocks to Taiko's inbox contract i.e `TaikoL1`
- Handles slashing of preconfers for not respecting preconfirmations.
- Manages the L1 validator-lookahead.
  - It is the responsibility of the first preconfer is every epoch to post the validator-lookahead for the next epoch.
  - This contract also contains the logic for processing a fault-proof for an incorrect validator-lookahead.
- Selects a fallback preconfer when an epoch does not have a single validator that is owned by a registered preconfer, or when the lookahead for the current epoch is invalidated.

### PreconfServiceManager

- The address of this contract is "AVS address" as in the address of our service on the restaking platform.
- This contract will be given the rights to slash a preconfer for malevolant behaviour.

> [!IMPORTANT]
> While we have based the service manager off a mock restaking service inspired by Eigenlayer, we have kept the design flexible enough to be adjusted to any in-production restaking service in the next iteration.
