// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ITaikoL1} from "../interfaces/taiko/ITaikoL1.sol";
import {MerkleUtils} from "../libraries/MerkleUtils.sol";
import {EIP4788} from "../libraries/EIP4788.sol";
import {PreconfConstants} from "./libraries/PreconfConstants.sol";
import {IPreconfTaskManager} from "../interfaces/IPreconfTaskManager.sol";
import {IPreconfServiceManager} from "../interfaces/IPreconfServiceManager.sol";
import {IPreconfRegistry} from "../interfaces/IPreconfRegistry.sol";
import {ECDSA} from "openzeppelin-contracts/utils/cryptography/ECDSA.sol";
import {IERC20} from "openzeppelin-contracts/token/ERC20/IERC20.sol";
import {Initializable} from "openzeppelin-contracts-upgradeable/proxy/utils/Initializable.sol";

contract PreconfTaskManager is IPreconfTaskManager, Initializable {
    IPreconfServiceManager internal immutable preconfServiceManager;
    IPreconfRegistry internal immutable preconfRegistry;
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

    constructor(
        IPreconfServiceManager _serviceManager,
        IPreconfRegistry _registry,
        ITaikoL1 _taikoL1,
        address _beaconBlockRootContract
    ) {
        preconfServiceManager = _serviceManager;
        preconfRegistry = _registry;
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
        uint256 currentEpochTimestamp = _getEpochTimestamp(block.timestamp);
        address randomPreconfer = randomPreconfers[currentEpochTimestamp];

        // Verify that the sender is allowed to propose a block in this slot

        if (randomPreconfer != address(0) && msg.sender != randomPreconfer) {
            // Revert if the sender is not the randomly selected preconfer for the epoch
            revert IPreconfTaskManager.SenderIsNotTheFallbackPreconfer();
        } else if (isLookaheadRequired(currentEpochTimestamp) || block.timestamp < lookahead[lookaheadTail].timestamp) {
            // A fallback preconfer is selected randomly for the current epoch when
            // - Lookahead is empty i.e the epoch has no L1 validators who are opted-in preconfers in the AVS
            // - The previous lookahead for the epoch was invalidated
            // - It is the first epoch after this contract started offering services

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
     * @notice Proves that the lookahead for a specific slot was incorrect
     * @dev The logic in this function only works once the lookahead slot has passed. This is because
     * we pull the proposer from a beacon block and verify if it is associated with the preconfer.
     * @param lookaheadPointer The pointer to the lookahead entry that represents the incorrect slot
     * @param slotTimestamp The timestamp of the slot for which the lookahead was incorrect
     * @param validatorBLSPubKey The BLS public key of the validator who is proposed the block in the slot
     * @param validatorInclusionProof The inclusion proof of the above validator in the Beacon state
     */
    function proveIncorrectLookahead(
        uint256 lookaheadPointer,
        uint256 slotTimestamp,
        bytes memory validatorBLSPubKey,
        EIP4788.InclusionProof memory validatorInclusionProof
    ) external {
        uint256 epochTimestamp = _getEpochTimestamp(slotTimestamp);

        // The poster must not already be slashed
        if (lookaheadPosters[epochTimestamp] == address(0)) {
            revert IPreconfTaskManager.PosterAlreadySlashedForTheEpoch();
        }

        // Must not have missed dispute period
        if (block.timestamp - slotTimestamp > DISPUTE_PERIOD) {
            revert IPreconfTaskManager.MissedDisputeWindow();
        }

        // Verify that the sent validator is the one in Beacon state
        EIP4788.verifyValidator(validatorBLSPubKey, _getBeaconBlockRoot(slotTimestamp), validatorInclusionProof);

        // We pull the preconfer present at the required slot timestamp in the lookahead.
        // If no preconfer is present for a slot, we simply use the 0-address to denote the preconfer.

        address preconferInLookahead;
        if (randomPreconfers[epochTimestamp] != address(0)) {
            // If the epoch had a random preconfer, the lookahead was empty
            preconferInLookahead = address(0);
        } else {
            IPreconfTaskManager.LookaheadEntry memory lookaheadEntry =
                lookahead[lookaheadPointer % LOOKAHEAD_BUFFER_SIZE];

            // Validate lookahead pointer
            if (slotTimestamp > lookaheadEntry.timestamp || slotTimestamp <= lookaheadEntry.prevTimestamp) {
                revert IPreconfTaskManager.InvalidLookaheadPointer();
            }

            if (lookaheadEntry.timestamp == slotTimestamp) {
                // The slot was dedicated to a specific preconfer
                preconferInLookahead = lookaheadEntry.preconfer;
            } else {
                // The slot was empty and it was the next preconfer who was expected to preconf in advanced.
                // We still use the zero address because technically the slot itself was empty in the lookahead.
                preconferInLookahead = address(0);
            }
        }

        // Fetch the preconfer associated with the validator from the registry

        // Reduce validator's BLS pub key to the pub key hash expected by the registry
        bytes32 validatorPubKeyHash = keccak256(abi.encodePacked(bytes16(0), validatorBLSPubKey));

        // Retrieve the validator object
        IPreconfRegistry.Validator memory validatorInRegistry = preconfRegistry.getValidator(validatorPubKeyHash);

        // Retrieve the preconfer
        address preconferInRegistry = validatorInRegistry.preconfer;
        if (
            slotTimestamp < validatorInRegistry.startProposingAt
                || (validatorInRegistry.stopProposingAt != 0 && slotTimestamp > validatorInRegistry.stopProposingAt)
        ) {
            // The validator is no longer allowed to propose for the former preconfer
            preconferInRegistry = address(0);
        }

        // Revert if the lookahead preconfer matches the one that the validator pulled from beacon state
        // is proposing for
        if (preconferInLookahead == preconferInRegistry) {
            revert IPreconfTaskManager.LookaheadEntryIsCorrect();
        }

        // Slash the original lookahead poster
        address poster = lookaheadPosters[epochTimestamp];
        lookaheadPosters[epochTimestamp] = address(0);
        preconfServiceManager.slashOperator(poster);

        emit ProvedIncorrectLookahead(poster, slotTimestamp, msg.sender);
    }

    //=========
    // Helpers
    //=========

    /// @dev Updates the lookahead for the next epoch
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

            // Each entry must be registered in the preconf registry
            if (preconfRegistry.getPreconferIndex(preconfer) != 0) {
                revert IPreconfTaskManager.PreconferNotRegistered();
            }

            // Ensure that the timestamps belong to a valid slot in the next epoch
            if (
                (slotTimestamp - nextEpochTimestamp) % 12 != 0 || slotTimestamp >= nextEpochEndTimestamp
                    || slotTimestamp <= prevSlotTimestamp
            ) {
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
        uint256 preconferIndex = randomness % preconfRegistry.getNextPreconferIndex();
        return preconfRegistry.getPreconferAtIndex(preconferIndex);
    }

    function isLookaheadRequired(uint256 epochTimestamp) public view returns (bool) {
        return lookaheadPosters[epochTimestamp] == address(0);
    }
}
