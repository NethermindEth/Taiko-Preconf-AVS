// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

interface PreconfTaskManager {
    event ProvedIncorrectPreconfirmation(address indexed preconfer, uint256 indexed blockId, address indexed disputer);
    event ProvedIncorrectLookahead(address indexed poster, uint256 indexed slot, address indexed disputer);

    struct LookaheadEntry {
        // The timestamp of the slot
        uint256 timestamp;
        // The id of the AVS operator who is also the L1 validator for the slot
        uint256 validatorId;
    }

    /// @dev Accepts block proposal by an operator and forwards it to TaikoL1 contract
    function newBlockProposal(
        bytes calldata blockParams,
        bytes calldata txList,
        LookaheadEntry[] calldata lookaheadEntries
    ) external;

    /// @dev Slashes a preconfer if the txn and ordering in a signed preconf does not match the actual block
    function proveIncorrectPreconfirmation(uint256 blockId, bytes32 txListHash, bytes memory signature) external;

    /// @dev Slashes a preconfer if the validator lookahead pushed by them has an incorrect entry
    function proveIncorrectLookahead(
        uint256 offset,
        bytes32[] memory expectedValidator,
        uint256 expectedValidatorIndex,
        bytes32[] memory expectedValidatorProof,
        bytes32[] memory actualValidator,
        uint256 actualValidatorIndex,
        bytes32[] memory actualValidatorProof,
        bytes32 validatorsRoot,
        uint256 nr_validators,
        bytes32[] memory beaconStateProof,
        bytes32 beaconStateRoot,
        bytes32[] memory beaconBlockProof
    ) external;
}
