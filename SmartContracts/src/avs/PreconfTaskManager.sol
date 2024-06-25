// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ITaikoL1} from "../interfaces/taiko/ITaikoL1.sol";
import {IPreconfTaskManager} from "../interfaces/IPreconfTaskManager.sol";
import {IPreconfServiceManager} from "../interfaces/IPreconfServiceManager.sol";
import {IRegistryCoordinator} from "eigenlayer-middleware/interfaces/IRegistryCoordinator.sol";
import {IIndexRegistry} from "eigenlayer-middleware/interfaces/IIndexRegistry.sol";
import {ECDSA} from "openzeppelin-contracts/utils/cryptography/ECDSA.sol";

contract PreconfTaskManager is IPreconfTaskManager {
    IPreconfServiceManager internal immutable preconfServiceManager;
    IRegistryCoordinator internal immutable registryCoordinator;
    IIndexRegistry internal immutable indexRegistry;
    ITaikoL1 internal immutable taikoL1;

    // Dec-01-2020 12:00:23 PM +UTC
    uint256 internal constant BEACON_GENESIS_TIMESTAMP = 1606824023;
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

    constructor(
        IPreconfServiceManager _serviceManager,
        IRegistryCoordinator _registryCoordinator,
        IIndexRegistry _indexRegistry,
        ITaikoL1 _taikoL1
    ) {
        preconfServiceManager = _serviceManager;
        registryCoordinator = _registryCoordinator;
        indexRegistry = _indexRegistry;
        taikoL1 = _taikoL1;

        nextBlockId = 1;
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
        uint256 currentEpochTimestamp = _getEpochTimestamp();
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
    ) external {}

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
                revert IPreconfTaskManager.SenderNotRegisteredInAVS();
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
     * @notice Computes the timestamp at which the ongoing epoch started.
     */
    function _getEpochTimestamp() private view returns (uint256) {
        uint256 timePassedSinceGenesis = block.timestamp - BEACON_GENESIS_TIMESTAMP;
        uint256 timeToCurrentEpochFromGenesis = (timePassedSinceGenesis / SECONDS_IN_EPOCH) * SECONDS_IN_EPOCH;
        return BEACON_GENESIS_TIMESTAMP + timeToCurrentEpochFromGenesis;
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
