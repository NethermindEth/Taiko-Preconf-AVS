// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

interface IPreconfTaskManager {
    struct ProposedBlock {
        // Proposer of the L2 block
        address proposer;
        // L1 block timestamp
        uint96 timestamp;
        // Keccak hash of the RLP transaction list of the block
        bytes32 txListHash;
    }

    struct PreconfirmationHeader {
        // The block height for which the preconfirmation is provided
        uint256 blockId;
        // The chain id of the target chain on which the preconfirmed transactions are settled
        uint256 chainId;
        // The keccak hash of the RLP encoded transaction list
        bytes32 txListHash;
    }

    struct LookaheadEntry {
        // Timestamp of the slot at which the provided preconfer is the L1 validator
        uint48 timestamp;
        // Timestamp of the last slot that had a valid preconfer
        uint48 prevTimestamp;
        // Address of the preconfer who is also the L1 validator
        // The preconfer will have rights to propose a block in the range (prevTimestamp, timestamp]
        address preconfer;
    }

    struct LookaheadSetParam {
        // The timestamp of the slot
        uint256 timestamp;
        // The AVS operator who is also the L1 validator for the slot and will preconf L2 transactions
        address preconfer;
    }

    event LookaheadUpdated(LookaheadSetParam[]);
    event ProvedIncorrectPreconfirmation(address indexed preconfer, uint256 indexed blockId, address indexed disputer);
    event ProvedIncorrectLookahead(address indexed poster, uint256 indexed slot, address indexed disputer);

    /// @dev The block proposer is not the randomly chosen fallback preconfer for the current slot/timestamp
    error SenderIsNotTheFallbackPreconfer();
    /// @dev The current timestamp does not fall in the range provided by the lookahead pointer
    error InvalidLookaheadPointer();
    /// @dev The block proposer is not the assigned preconfer for the current slot/timestamp
    error SenderIsNotThePreconfer();
    /// @dev The preconfer in the lookahead set params is not registered to the AVS
    error SenderNotRegisteredInAVS();
    /// @dev The timestamp in the lookahead is not of a valid future slot in the present epoch
    error InvalidSlotTimestamp();
    /// @dev The chain id on which the preconfirmation was signed is different from the current chain's id
    error PreconfirmationChainIdMismatch();
    /// @dev The dispute window for proving incorretc lookahead or preconfirmation is over
    error MissedDisputeWindow();
    /// @dev The disputed preconfirmation is correct
    error PreconfirmationIsCorrect();

    /// @dev Accepts block proposal by an operator and forwards it to TaikoL1 contract
    function newBlockProposal(
        bytes calldata blockParams,
        bytes calldata txList,
        uint256 lookaheadHint,
        LookaheadSetParam[] calldata lookaheadSetParams
    ) external;

    /// @dev Slashes a preconfer if the txn and ordering in a signed preconf does not match the actual block
    function proveIncorrectPreconfirmation(PreconfirmationHeader memory header, bytes memory signature) external;

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

    function isLookaheadRequired(uint256 epochTimestamp) external view returns (bool);
}
