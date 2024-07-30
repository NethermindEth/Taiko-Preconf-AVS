// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ITaikoL1} from "../interfaces/taiko/ITaikoL1.sol";
import {MerkleUtils} from "../libraries/MerkleUtils.sol";
import {IPreconfTaskManager} from "../interfaces/IPreconfTaskManager.sol";
import {IPreconfServiceManager} from "../interfaces/IPreconfServiceManager.sol";
import {IRegistryCoordinator} from "eigenlayer-middleware/interfaces/IRegistryCoordinator.sol";
import {IIndexRegistry} from "eigenlayer-middleware/interfaces/IIndexRegistry.sol";
import {ECDSA} from "openzeppelin-contracts/utils/cryptography/ECDSA.sol";
import {IERC20} from "openzeppelin-contracts/token/ERC20/IERC20.sol";
import {Initializable} from "openzeppelin-contracts-upgradeable/proxy/utils/Initializable.sol";

contract PreconfTaskManager is IPreconfTaskManager, Initializable {
    IPreconfServiceManager internal immutable preconfServiceManager;
    IRegistryCoordinator internal immutable registryCoordinator;
    IIndexRegistry internal immutable indexRegistry;
    ITaikoL1 internal immutable taikoL1;

    // EIP-4788
    address internal immutable beaconBlockRootContract;

    // Dec-01-2020 12:00:23 PM +UTC
    uint256 internal constant BEACON_GENESIS_TIMESTAMP = 1606824023;
    uint256 internal constant SECONDS_IN_SLOT = 12;
    // 12 seconds for each slot, with 32 slots in each epoch
    uint256 internal constant SECONDS_IN_EPOCH = 384;
    // Span time within which a preconfirmation or posted lookahead may be disputed
    uint256 internal constant DISPUTE_PERIOD = 2 * SECONDS_IN_EPOCH;

    // A ring buffer of upcoming preconfers (who are also the L1 validators)
    uint256 internal lookaheadTail;
    uint256 internal constant LOOKAHEAD_BUFFER_SIZE = 64;
    IPreconfTaskManager.LookaheadEntry[LOOKAHEAD_BUFFER_SIZE] internal lookahead;

    // Todo: Make the below two data structure more efficient by reusing slots

    // Current and past lookahead posters as a mapping indexed by the timestamp of the epoch
    mapping(uint256 epochTimestamp => address poster) internal lookaheadPosters;
    // Maps the epoch timestamp to the randomly selected preconfer (if present) for that epoch
    mapping(uint256 epochTimestamp => address randomPreconfer) internal randomPreconfers;

    // A ring buffer of proposed (and preconfed) L2 blocks
    uint256 nextBlockId;
    uint256 internal constant PROPOSED_BLOCK_BUFFER_SIZE = 256;
    mapping(uint256 blockId => IPreconfTaskManager.ProposedBlock proposedBlock) proposedBlocks;

    // Maps the preconfer's wallet address to the hash tree root of their consensus layer
    // BLS pub key (48  bytes)
    mapping(address preconfer => bytes32 BLSPubKeyHashTreeRoot) internal consensusBLSPubKeyHashTreeRoots;

    constructor(
        IPreconfServiceManager _serviceManager,
        IRegistryCoordinator _registryCoordinator,
        IIndexRegistry _indexRegistry,
        ITaikoL1 _taikoL1,
        address _beaconBlockRootContract
    ) {
        preconfServiceManager = _serviceManager;
        registryCoordinator = _registryCoordinator;
        indexRegistry = _indexRegistry;
        taikoL1 = _taikoL1;
        beaconBlockRootContract = _beaconBlockRootContract;
    }

    function initialize(IERC20 _taikoToken) external initializer {
        nextBlockId = 1;
        _taikoToken.approve(address(taikoL1), type(uint256).max);
    }

    /**
     * @notice Proposes a new Taiko L2 block.
     * @dev This may either be called by a randomly selected preconfer or by a preconfer expected for the current slot
     * as per the lookahead. The first caller in every is expected to pass along the lookahead entries for the next epoch.
     * @param blockParams Block parameters expected by TaikoL1 contract
     * @param txList RLP encoded transaction list expected by TaikoL1 contract
     * @param lookaheadPointer A pointer to the lookahead entry that may prove that the sender is the preconfer
     * for the slot.
     * @param lookaheadSetParams Collection of timestamps and preconfer addresses to be inserted in the lookahead
     */
    function newBlockProposal(
        bytes calldata blockParams,
        bytes calldata txList,
        uint256 lookaheadPointer,
        IPreconfTaskManager.LookaheadSetParam[] calldata lookaheadSetParams
    ) external payable {
        // Do not allow block proposals if the preconfer has not registered the hash tree root of their
        // consensus BLS pub key
        if (consensusBLSPubKeyHashTreeRoots[msg.sender] == bytes32(0)) {
            revert IPreconfTaskManager.ConsensusBLSPubKeyHashTreeRootNotRegistered();
        }

        uint256 currentEpochTimestamp = _getEpochTimestamp(block.timestamp);
        address randomPreconfer = randomPreconfers[currentEpochTimestamp];

        // Verify that the sender is a valid preconfer for the slot and has the right to propose an L2 block
        if (randomPreconfer != address(0) && msg.sender != randomPreconfer) {
            // Revert if the sender is not the randomly selected preconfer for the epoch
            revert IPreconfTaskManager.SenderIsNotTheFallbackPreconfer();
        } else if (isLookaheadRequired(currentEpochTimestamp)) {
            // The *current* epoch may require a lookahead in the following situations:
            // - It is the first epoch after this contract started offering services
            // - The epoch has no L1 validators who are opted-in preconfers in the AVS
            // - The previous lookahead for the epoch was invalidated/
            //
            // In all the above cases, we expect a preconfer to be randomly chosen as fallaback
            if (msg.sender != getFallbackPreconfer()) {
                revert IPreconfTaskManager.SenderIsNotTheFallbackPreconfer();
            } else {
                randomPreconfers[currentEpochTimestamp] = msg.sender;
            }
        } else {
            IPreconfTaskManager.LookaheadEntry memory lookaheadEntry =
                lookahead[lookaheadPointer % LOOKAHEAD_BUFFER_SIZE];

            // The current L1 block's timestamp must be within the range retrieved from the lookahead entry.
            // The preconfer is allowed to propose a block in advanced if there are no other entries in the
            // lookahead between the present slot and the preconfer's own slot.
            //
            // ------[Last slot with an entry]---[X]---[X]----[X]----[Preconfer]-------
            // ------[     prevTimestamp     ]---[ ]---[ ]----[ ]----[timestamp]-------
            //
            if (block.timestamp <= lookaheadEntry.prevTimestamp || block.timestamp > lookaheadEntry.timestamp) {
                revert IPreconfTaskManager.InvalidLookaheadPointer();
            } else if (msg.sender != lookaheadEntry.preconfer) {
                revert IPreconfTaskManager.SenderIsNotThePreconfer();
            }
        }

        uint256 nextEpochTimestamp = currentEpochTimestamp + SECONDS_IN_EPOCH;

        // Update the lookahead for the next epoch.
        // Only called during the first block proposal of the current epoch.
        if (isLookaheadRequired(nextEpochTimestamp)) {
            _updateLookahead(currentEpochTimestamp, lookaheadSetParams);
        }

        uint256 _nextBlockId = nextBlockId;

        // Store the hash of the transaction list and the proposer of the proposed block.
        // The hash is later used to verify transaction inclusion/ordering in a preconfirmation.
        proposedBlocks[_nextBlockId % PROPOSED_BLOCK_BUFFER_SIZE] = IPreconfTaskManager.ProposedBlock({
            proposer: msg.sender,
            timestamp: uint96(block.timestamp),
            txListHash: keccak256(txList)
        });

        nextBlockId = _nextBlockId + 1;

        // Block the preconfer from withdrawing stake from Eigenlayer during the dispute window
        preconfServiceManager.lockStakeUntil(msg.sender, block.timestamp + DISPUTE_PERIOD);

        // Forward the block to Taiko's L1 contract
        taikoL1.proposeBlock{value: msg.value}(blockParams, txList);
    }

    /**
     * @notice Slashes an operator if their preconfirmation has not been respected onchain
     * @dev The ECDSA signature expected by this function must be from a library that prevents malleable
     * signatures i.e `s` value is in the lower half order, and the `v` value is either 27 or 28.
     * @param header The header of the preconfirmation sent to the AVS P2P
     * @param signature ECDSA-signed hash of the preconfirmation header
     */
    function proveIncorrectPreconfirmation(PreconfirmationHeader calldata header, bytes calldata signature) external {
        IPreconfTaskManager.ProposedBlock memory proposedBlock = proposedBlocks[header.blockId];

        if (block.timestamp - proposedBlock.timestamp >= DISPUTE_PERIOD) {
            // Revert if the dispute window has been missed
            revert IPreconfTaskManager.MissedDisputeWindow();
        } else if (header.chainId != block.chainid) {
            // Revert if the preconfirmation was provided on another chain
            revert IPreconfTaskManager.PreconfirmationChainIdMismatch();
        }

        bytes32 headerHash = keccak256(abi.encodePacked(header.blockId, header.chainId, header.txListHash));
        address preconfSigner = ECDSA.recover(headerHash, signature);

        // Note: It is not required to verify that the preconfSigner is a valid operator. That is implicitly
        // verified by EL.

        // Slash if the preconfirmation was given offchain, but block proposal was missed OR
        // the preconfirmed set of transactions is different from the transactions in the proposed block.
        if (preconfSigner != proposedBlock.proposer || header.txListHash != proposedBlock.txListHash) {
            preconfServiceManager.slashOperator(preconfSigner);
        } else {
            revert IPreconfTaskManager.PreconfirmationIsCorrect();
        }
    }

    /**
     * @notice lashes an operator if the lookahead posted by them is incorrect
     * @param lookaheadPointer Index at which the entry for the lookahead is incorrect
     * @param slotTimestamp Timestamp of the slot for which the lookahead entry is incorrect
     * @param expectedValidator Chunks for the validator present in the lookahead
     * @param expectedValidatorIndex Index of the expected validator in beacon state validators list
     * @param expectedValidatorProof Merkle proof of expected validator being a part of validators list
     * @param actualValidatorIndex Index of the actual validator in beacon state validators list
     * @param validatorsRoot Hash tree root of the beacon state validators list
     * @param nr_validators Length of the validators list
     * @param beaconStateProof Merkle proof of validators list being a part of the beacon state
     * @param beaconStateRoot Hash tree root of the beacon state
     * @param beaconBlockProofForState Merkle proof of beacon state root being a part of the beacon block
     * @param beaconBlockProofForProposerIndex Merkle proof of actual validator's index being a part of the beacon block
     */
    function proveIncorrectLookahead(
        uint256 lookaheadPointer,
        uint256 slotTimestamp,
        bytes32[8] memory expectedValidator,
        uint256 expectedValidatorIndex,
        bytes32[] memory expectedValidatorProof,
        uint256 actualValidatorIndex,
        bytes32 validatorsRoot,
        uint256 nr_validators,
        bytes32[] memory beaconStateProof,
        bytes32 beaconStateRoot,
        bytes32[] memory beaconBlockProofForState,
        bytes32[] memory beaconBlockProofForProposerIndex
    ) external {
        // Prove that the expected validator has not been slashed on consensus layer
        if (expectedValidator[3] == bytes32(0)) {
            revert IPreconfTaskManager.ExpectedValidatorMustNotBeSlashed();
        }

        uint256 epochTimestamp = _getEpochTimestamp(slotTimestamp);

        // The poster must not already be slashed
        if (lookaheadPosters[epochTimestamp] == address(0)) {
            revert IPreconfTaskManager.PosterAlreadySlashedForTheEpoch();
        }

        {
            IPreconfTaskManager.LookaheadEntry memory lookaheadEntry =
                lookahead[lookaheadPointer % LOOKAHEAD_BUFFER_SIZE];

            // Timestamp of the slot that contains the incorrect entry must be in the correct range and within the
            // dispute window.
            if (block.timestamp - slotTimestamp > DISPUTE_PERIOD) {
                revert IPreconfTaskManager.MissedDisputeWindow();
            } else if (slotTimestamp > lookaheadEntry.timestamp || slotTimestamp <= lookaheadEntry.prevTimestamp) {
                revert IPreconfTaskManager.InvalidLookaheadPointer();
            }

            if (consensusBLSPubKeyHashTreeRoots[lookaheadEntry.preconfer] != expectedValidator[0]) {
                // Revert if the expected validator's consensus BLS pub key's hash tree root does not match
                // the one registered by the preconfer
                revert IPreconfTaskManager.ExpectedValidatorIsIncorrect();
            } else if (expectedValidatorIndex == actualValidatorIndex) {
                revert IPreconfTaskManager.ExpectedAndActualValidatorAreSame();
            }
        }

        {
            bytes32 expectedValidatorHashTreeRoot = MerkleUtils.merkleize(expectedValidator);
            if (
                !MerkleUtils.verifyProof(
                    expectedValidatorProof, validatorsRoot, expectedValidatorHashTreeRoot, expectedValidatorIndex
                )
            ) {
                // Revert if the proof that the expected validator is a part of the validator
                // list in beacon state fails
                revert IPreconfTaskManager.ValidatorProofFailed();
            }
        }

        {
            bytes32 stateValidatorsHashTreeRoot = MerkleUtils.mixInLength(validatorsRoot, nr_validators);
            if (MerkleUtils.verifyProof(beaconStateProof, beaconStateRoot, stateValidatorsHashTreeRoot, 11)) {
                // Revert if the proof that the validator list is a part of the beacon state fails
                revert IPreconfTaskManager.BeaconStateProofFailed();
            }
        }

        bytes32 beaconBlockRoot = _getBeaconBlockRoot(slotTimestamp);

        if (MerkleUtils.verifyProof(beaconBlockProofForState, beaconBlockRoot, beaconStateRoot, 3)) {
            // Revert if the proof for the beacon state being a part of the beacon block fails
            revert IPreconfTaskManager.BeaconBlockProofForStateFailed();
        }

        if (
            MerkleUtils.verifyProof(
                beaconBlockProofForProposerIndex, beaconBlockRoot, MerkleUtils.toLittleEndian(actualValidatorIndex), 1
            )
        ) {
            // Revert if the proof that the proposer index is a part of the beacon block fails
            revert IPreconfTaskManager.BeaconBlockProofForProposerIndex();
        }

        // Slash the original lookahead poster
        address poster = lookaheadPosters[epochTimestamp];
        lookaheadPosters[epochTimestamp] = address(0);
        preconfServiceManager.slashOperator(poster);

        emit ProvedIncorrectLookahead(poster, slotTimestamp, msg.sender);
    }

    function registerConsensusBLSPubKeyHashTreeRoot(bytes32 consensusBLSPubKeyHashTreeRoot) external {
        // The sender must be a registered preconfer in the AVS registry
        if (registryCoordinator.getOperatorStatus(msg.sender) != IRegistryCoordinator.OperatorStatus.REGISTERED) {
            revert IPreconfTaskManager.SenderNotRegisteredInAVS();
        } else if (consensusBLSPubKeyHashTreeRoots[msg.sender] != bytes32(0)) {
            // The hash tree root must not be already registered
            revert IPreconfTaskManager.SenderNotRegisteredInAVS();
        }

        consensusBLSPubKeyHashTreeRoots[msg.sender] = consensusBLSPubKeyHashTreeRoot;
    }

    //=========
    // Helpers
    //=========

    function _updateLookahead(
        uint256 epochTimestamp,
        IPreconfTaskManager.LookaheadSetParam[] calldata lookaheadSetParams
    ) private {
        uint256 nextEpochTimestamp = epochTimestamp + SECONDS_IN_EPOCH;
        uint256 nextEpochEndTimestamp = nextEpochTimestamp + SECONDS_IN_EPOCH;

        // The tail of the lookahead is tracked and connected to the first new lookahead entry so
        // that when no more preconfers are present in the remaining slots of the current epoch,
        // the next epoch's preconfer may start preconfing in advanced.
        //
        // --[]--[]--[p1]--[]--[]---|---[]--[]--[P2]--[]--[]
        //   1   2    3    4   5        6    7    8   9   10
        //         Epoch 1                     Epoch 2
        //
        // Here, P2 may start preconfing and proposing blocks from slot 4 itself
        //
        uint256 _lookaheadTail = lookaheadTail;
        uint256 prevSlotTimestamp = lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].timestamp;

        for (uint256 i; i < lookaheadSetParams.length; ++i) {
            _lookaheadTail += 1;

            address preconfer = lookaheadSetParams[i].preconfer;
            uint256 slotTimestamp = lookaheadSetParams[i].timestamp;

            // Each entry must be a registered AVS operator
            if (registryCoordinator.getOperatorStatus(preconfer) != IRegistryCoordinator.OperatorStatus.REGISTERED) {
                revert IPreconfTaskManager.EntryNotRegisteredInAVS();
            }

            // Ensure that the timestamps belong to a valid slot in the next epoch
            if ((slotTimestamp - nextEpochTimestamp) % 12 != 0 || slotTimestamp >= nextEpochEndTimestamp) {
                revert IPreconfTaskManager.InvalidSlotTimestamp();
            }

            // Update the lookahead entry
            lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE] = IPreconfTaskManager.LookaheadEntry({
                timestamp: uint48(slotTimestamp),
                prevTimestamp: uint48(prevSlotTimestamp),
                preconfer: preconfer
            });
            prevSlotTimestamp = slotTimestamp;
        }

        lookaheadTail = _lookaheadTail;
        lookaheadPosters[epochTimestamp] = msg.sender;

        emit LookaheadUpdated(lookaheadSetParams);
    }

    /**
     * @notice Computes the timestamp of the epoch containing the provided slot timestamp
     */
    function _getEpochTimestamp(uint256 slotTimestamp) private pure returns (uint256) {
        uint256 timePassedSinceGenesis = slotTimestamp - BEACON_GENESIS_TIMESTAMP;
        uint256 timeToCurrentEpochFromGenesis = (timePassedSinceGenesis / SECONDS_IN_EPOCH) * SECONDS_IN_EPOCH;
        return BEACON_GENESIS_TIMESTAMP + timeToCurrentEpochFromGenesis;
    }

    /**
     * @notice Retrieves the beacon block root for the block at the specified timestamp
     */
    function _getBeaconBlockRoot(uint256 timestamp) private view returns (bytes32) {
        // At block N, we get the beacon block root for block N - 1. So, to get the block root of the Nth block,
        // we query the root at block N + 1. If N + 1 is a missed slot, we keep querying until we find a block N + x
        // that has the block root for Nth block.
        uint256 targetTimestamp = timestamp + SECONDS_IN_SLOT;
        while (true) {
            (bool success, bytes memory result) = beaconBlockRootContract.staticcall(abi.encode(targetTimestamp));
            if (success && result.length > 0) {
                return abi.decode(result, (bytes32));
            }

            unchecked {
                targetTimestamp += SECONDS_IN_SLOT;
            }
        }
        return bytes32(0);
    }

    //=======
    // Views
    //=======

    function getFallbackPreconfer() public view returns (address) {
        uint256 randomness = block.prevrandao;
        // For the POC we only have one quorum at the 0th id. If there are multiple quorums in productions
        // we may also need to randomise the quorum selection.
        uint8 quorumNumber = 0;
        uint32 operatorIndex = uint32(randomness % indexRegistry.totalOperatorsForQuorum(quorumNumber));
        bytes32 operatorId = bytes32(indexRegistry.getLatestOperatorUpdate(quorumNumber, operatorIndex).operatorId);
        return registryCoordinator.getOperatorFromId(operatorId);
    }

    function isLookaheadRequired(uint256 epochTimestamp) public view returns (bool) {
        return lookaheadPosters[epochTimestamp] == address(0);
    }
}
