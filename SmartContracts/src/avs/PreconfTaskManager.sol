// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ITaikoL1} from "../interfaces/taiko/ITaikoL1.sol";
import {EIP4788} from "../libraries/EIP4788.sol";
import {PreconfConstants} from "./PreconfConstants.sol";
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
    uint256 internal immutable beaconGenesis;
    address internal immutable beaconBlockRootContract;

    // A ring buffer of upcoming preconfers (who are also the L1 validators)
    uint256 internal lookaheadTail;
    uint256 internal constant LOOKAHEAD_BUFFER_SIZE = 64;
    LookaheadBufferEntry[LOOKAHEAD_BUFFER_SIZE] internal lookahead;

    // Maps the epoch timestamp to the lookahead poster.
    // If the lookahead poster has been slashed, it maps to the 0-address.
    // Note: This may be optimised to re-use existing slots and reduce gas cost.
    mapping(uint256 epochTimestamp => address poster) internal lookaheadPosters;

    // Maps the block height to the associated proposer
    // This is required since the stored block in Taiko has the address of this contract as the proposer
    mapping(uint256 blockId => address proposer) internal blockIdToProposer;

    // Cannot be kept in `PreconfConstants` file because solidity expects array sizes
    // to be stored in the main contract file itself.
    uint256 internal constant SLOTS_IN_EPOCH = 32;

    constructor(
        IPreconfServiceManager _serviceManager,
        IPreconfRegistry _registry,
        ITaikoL1 _taikoL1,
        uint256 _beaconGenesis,
        address _beaconBlockRootContract
    ) {
        preconfServiceManager = _serviceManager;
        preconfRegistry = _registry;
        taikoL1 = _taikoL1;
        beaconGenesis = _beaconGenesis;
        beaconBlockRootContract = _beaconBlockRootContract;
    }

    function initialize(IERC20 _taikoToken) external initializer {
        _taikoToken.approve(address(taikoL1), type(uint256).max);
    }

    /**
     * @notice Proposes a new Taiko L2 block.
     * @dev The first caller in every epoch is expected to pass along the lookahead entries for the next epoch.
     * The function reverts if the lookahead is lagging behind. This is possible if it is
     * the first block proposal of the system or no lookahead was posted for the current epoch due to missed proposals.
     * In this case, `forcePushLookahead` must be called in order to update the lookahead for the next epoch.
     * @param blockParams Array of block parameters expected by TaikoL1 contract
     * @param txLists Array of RLP encoded transaction lists expected by TaikoL1 contract
     * @param lookaheadPointer A pointer to the lookahead entry that may prove that the sender is the preconfer
     * for the slot.
     * @param lookaheadSetParams Collection of timestamps and preconfer addresses to be inserted in the lookahead
     */
    function newBlockProposal(
        bytes[] calldata blockParams,
        bytes[] calldata txLists,
        uint256 lookaheadPointer,
        LookaheadSetParam[] calldata lookaheadSetParams
    ) external payable {
        LookaheadBufferEntry memory lookaheadEntry = lookahead[lookaheadPointer % LOOKAHEAD_BUFFER_SIZE];

        uint256 currentEpochTimestamp = _getEpochTimestamp(block.timestamp);

        // The current L1 block's timestamp must be within the range retrieved from the lookahead entry.
        // The preconfer is allowed to propose a block in advanced if there are no other entries in the
        // lookahead between the present slot and the preconfer's own slot.
        //
        // ------[Last slot with an entry]---[X]---[X]----[X]----[Preconfer]-------
        // ------[     prevTimestamp     ]---[ ]---[ ]----[ ]----[timestamp]-------
        //
        if (block.timestamp <= lookaheadEntry.prevTimestamp || block.timestamp > lookaheadEntry.timestamp) {
            revert InvalidLookaheadPointer();
        } else if (msg.sender != lookaheadEntry.preconfer) {
            revert SenderIsNotThePreconfer();
        }

        uint256 nextEpochTimestamp = currentEpochTimestamp + PreconfConstants.SECONDS_IN_EPOCH;

        // Update the lookahead for the next epoch.
        // Only called during the first block proposal of the current epoch.
        if (isLookaheadRequired(nextEpochTimestamp)) {
            _updateLookahead(nextEpochTimestamp, lookaheadSetParams);
        }

        // Store the proposer for the block locally
        // Use Taiko's block number to index
        (, ITaikoL1.SlotB memory slotB) = taikoL1.getStateVariables();
        for (uint256 i = 0; i < blockParams.length; i++) {
            blockIdToProposer[slotB.numBlocks + i] = msg.sender;
        }

        // Block the preconfer from withdrawing stake from the restaking service during the dispute window
        preconfServiceManager.lockStakeUntil(msg.sender, block.timestamp + PreconfConstants.DISPUTE_PERIOD);

        // Forward the blocks to Taiko's L1 contract
        taikoL1.proposeBlocksV2{value: msg.value}(blockParams, txLists);
    }

    /**
     * @notice Proves that the preconfirmation for a specific block was not respected
     * @dev The function requires the metadata of the block in the format that Taiko uses. This is matched
     * against the metadata hash stored in Taiko.
     * @param taikoBlockMetadata The metadata of the Taiko block for which the preconfirmation was provided
     * @param header The header of the preconfirmation
     * @param signature The signature of the preconfirmation
     */
    function proveIncorrectPreconfirmation(
        ITaikoL1.BlockMetadataV2 calldata taikoBlockMetadata,
        PreconfirmationHeader calldata header,
        bytes calldata signature
    ) external {
        uint256 blockId = taikoBlockMetadata.id;
        address proposer = blockIdToProposer[blockId];

        // Pull the formalised block from Taiko
        ITaikoL1.BlockV2 memory taikoBlock = taikoL1.getBlockV2(uint64(blockId));

        if (block.timestamp - taikoBlock.proposedAt >= PreconfConstants.DISPUTE_PERIOD) {
            // Revert if the dispute window has been missed
            revert MissedDisputeWindow();
        } else if (header.chainId != block.chainid) {
            // Revert if the preconfirmation was provided on another chain
            revert PreconfirmationChainIdMismatch();
        } else if (keccak256(abi.encode(taikoBlockMetadata)) != taikoBlock.metaHash) {
            // Revert if the metadata of the block does not match the one stored in Taiko
            revert MetadataMismatch();
        }

        bytes32 headerHash = keccak256(abi.encodePacked(header.blockId, header.chainId, header.txListHash));
        address preconfSigner = ECDSA.recover(headerHash, signature);

        // Slash if the preconfirmation was given offchain, but block proposal was missed OR
        // the preconfirmed set of transactions is different from the transactions in the proposed block.
        if (preconfSigner != proposer || header.txListHash != taikoBlockMetadata.blobHash) {
            preconfServiceManager.slashOperator(preconfSigner);
        } else {
            revert PreconfirmationIsCorrect();
        }

        emit ProvedIncorrectPreconfirmation(proposer, blockId, msg.sender);
    }

    /**
     * @notice Proves that the lookahead for a specific slot was incorrect
     * @dev The logic in this function only works once the lookahead slot has passed. This is because
     * we pull the proposer from a past beacon block and verify if it is associated with the preconfer.
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

        address poster = lookaheadPosters[epochTimestamp];

        // Poster must not have been slashed
        if (poster == address(0)) {
            revert PosterAlreadySlashedOrLookaheadIsEmpty();
        }

        // Must not have missed dispute period
        if (block.timestamp - slotTimestamp > PreconfConstants.DISPUTE_PERIOD) {
            revert MissedDisputeWindow();
        }

        // Verify that the sent validator is the one in Beacon state
        EIP4788.verifyValidator(validatorBLSPubKey, _getBeaconBlockRoot(slotTimestamp), validatorInclusionProof);

        LookaheadBufferEntry memory lookaheadEntry = lookahead[lookaheadPointer % LOOKAHEAD_BUFFER_SIZE];

        // Validate lookahead pointer
        if (slotTimestamp > lookaheadEntry.timestamp || slotTimestamp <= lookaheadEntry.prevTimestamp) {
            revert InvalidLookaheadPointer();
        }

        // We pull the preconfer present at the required slot timestamp in the lookahead.
        // If no preconfer is present for a slot, we simply use the 0-address to denote the preconfer.
        address preconferInLookahead;
        if (lookaheadEntry.timestamp == slotTimestamp && !lookaheadEntry.isFallback) {
            // The slot was dedicated to a specific preconfer
            preconferInLookahead = lookaheadEntry.preconfer;
        } else {
            // The slot was empty and it was the next preconfer who was expected to preconf in advanced, OR
            // the slot was empty and the preconfer was expected to be the fallback preconfer for the epoch.
            // We still use the zero address because technically the slot itself was empty in the lookahead.
            preconferInLookahead = address(0);
        }

        // Reduce validator's BLS pub key to the pub key hash expected by the registry
        bytes32 validatorPubKeyHash = keccak256(abi.encodePacked(bytes16(0), validatorBLSPubKey));

        // Retrieve the validator object
        IPreconfRegistry.Validator memory validatorInRegistry = preconfRegistry.getValidator(validatorPubKeyHash);

        // Fetch the preconfer associated with the validator from the registry
        address preconferInRegistry = validatorInRegistry.preconfer;
        if (
            slotTimestamp < validatorInRegistry.startProposingAt
                || (validatorInRegistry.stopProposingAt != 0 && slotTimestamp >= validatorInRegistry.stopProposingAt)
        ) {
            // The validator is no longer allowed to propose for the former preconfer
            preconferInRegistry = address(0);
        }

        // Revert if the lookahead preconfer matches the one that the validator pulled from beacon state
        // is proposing for
        if (preconferInLookahead == preconferInRegistry) {
            revert LookaheadEntryIsCorrect();
        }

        uint256 epochEndTimestamp = epochTimestamp + PreconfConstants.SECONDS_IN_EPOCH;

        // If it is the current epoch's lookahead being proved incorrect then insert a fallback preconfer
        if (block.timestamp < epochEndTimestamp) {
            uint256 _lookaheadTail = lookaheadTail;

            uint256 lastSlotTimestamp = epochEndTimestamp - PreconfConstants.SECONDS_IN_SLOT;

            // If the lookahead for next epoch is available
            if (lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].timestamp >= epochEndTimestamp) {
                // Get to the entry in the next epoch that connects to a slot in the current epoch
                while (lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].prevTimestamp >= epochEndTimestamp) {
                    _lookaheadTail -= 1;
                }

                // Switch the connection to the last slot of the current epoch
                lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].prevTimestamp = uint40(lastSlotTimestamp);

                // Head to the last entry in current epoch
                _lookaheadTail -= 1;
            }

            lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE] = LookaheadBufferEntry({
                isFallback: true,
                timestamp: uint40(lastSlotTimestamp),
                prevTimestamp: uint40(epochTimestamp - PreconfConstants.SECONDS_IN_SLOT),
                preconfer: getFallbackPreconfer(epochTimestamp)
            });

            _lookaheadTail -= 1;

            // Nullify the rest of the lookahead entries for this epoch
            while (lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].timestamp >= epochTimestamp) {
                lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE] =
                    LookaheadBufferEntry({isFallback: false, timestamp: 0, prevTimestamp: 0, preconfer: address(0)});
                _lookaheadTail -= 1;
            }
        }

        // Slash the poster
        lookaheadPosters[epochTimestamp] = address(0);
        preconfServiceManager.slashOperator(poster);

        emit ProvedIncorrectLookahead(poster, slotTimestamp, msg.sender);
    }

    /**
     * @notice Forces the lookahead to be set for the next epoch if it is not already set.
     * @dev This is called once when the system starts up to push the first lookahead, and later anytime
     * when the lookahead is lagging due to missed proposals.
     * @param lookaheadSetParams Collection of timestamps and preconfer addresses to be inserted in the lookahead
     */
    function forcePushLookahead(LookaheadSetParam[] calldata lookaheadSetParams) external {
        // Sender must be a preconfer
        if (preconfRegistry.getPreconferIndex(msg.sender) == 0) {
            revert PreconferNotRegistered();
        }

        // Lookahead must be missing
        uint256 nextEpochTimestamp = _getEpochTimestamp(block.timestamp) + PreconfConstants.SECONDS_IN_EPOCH;
        if (!isLookaheadRequired(nextEpochTimestamp)) {
            revert LookaheadIsNotRequired();
        }

        // Update the lookahead for next epoch
        _updateLookahead(nextEpochTimestamp, lookaheadSetParams);

        // Block the preconfer from withdrawing stake from Eigenlayer during the dispute window
        preconfServiceManager.lockStakeUntil(msg.sender, block.timestamp + PreconfConstants.DISPUTE_PERIOD);
    }

    //=========
    // Helpers
    //=========

    /// @dev Updates the lookahead for an epoch
    function _updateLookahead(uint256 epochTimestamp, LookaheadSetParam[] calldata lookaheadSetParams) private {
        uint256 epochEndTimestamp = epochTimestamp + PreconfConstants.SECONDS_IN_EPOCH;

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

        if (lookaheadSetParams.length == 0) {
            // If no preconfers are present in the lookahead, we use the fallback preconfer for the entire epoch
            address fallbackPreconfer = getFallbackPreconfer(epochTimestamp);
            _lookaheadTail += 1;

            // and, insert it in the last slot of the epoch so that it may start preconfing in advanced
            lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE] = LookaheadBufferEntry({
                isFallback: true,
                timestamp: uint40(epochEndTimestamp - PreconfConstants.SECONDS_IN_SLOT),
                prevTimestamp: uint40(prevSlotTimestamp),
                preconfer: fallbackPreconfer
            });
        } else {
            for (uint256 i; i < lookaheadSetParams.length; ++i) {
                _lookaheadTail += 1;

                address preconfer = lookaheadSetParams[i].preconfer;
                uint256 slotTimestamp = lookaheadSetParams[i].timestamp;

                // Each entry must be registered in the preconf registry
                if (preconfRegistry.getPreconferIndex(preconfer) == 0) {
                    revert PreconferNotRegistered();
                }

                // Ensure that the timestamps belong to a valid slot in the epoch
                if (
                    (slotTimestamp - epochTimestamp) % 12 != 0 || slotTimestamp >= epochEndTimestamp
                        || slotTimestamp <= prevSlotTimestamp
                ) {
                    revert InvalidSlotTimestamp();
                }

                // Update the lookahead entry
                lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE] = LookaheadBufferEntry({
                    isFallback: false,
                    timestamp: uint40(slotTimestamp),
                    prevTimestamp: uint40(prevSlotTimestamp),
                    preconfer: preconfer
                });
                prevSlotTimestamp = slotTimestamp;
            }
        }

        lookaheadTail = _lookaheadTail;
        lookaheadPosters[epochTimestamp] = msg.sender;

        // We directly use the lookahead set params even in the case of a fallback preconfer to
        // assist the nodes in identifying an incorrect lookahead. The contents of this event can be matched against
        // the output of `getLookaheadParamsForEpoch` to verify the correctness of the lookahead.
        emit LookaheadUpdated(lookaheadSetParams);
    }

    /**
     * @notice Computes the timestamp of the epoch containing the provided slot timestamp
     */
    function _getEpochTimestamp(uint256 slotTimestamp) private view returns (uint256) {
        uint256 timePassedSinceGenesis = slotTimestamp - beaconGenesis;
        uint256 timeToCurrentEpochFromGenesis =
            (timePassedSinceGenesis / PreconfConstants.SECONDS_IN_EPOCH) * PreconfConstants.SECONDS_IN_EPOCH;
        return beaconGenesis + timeToCurrentEpochFromGenesis;
    }

    /**
     * @notice Retrieves the beacon block root for the block at the specified timestamp
     */
    function _getBeaconBlockRoot(uint256 timestamp) private view returns (bytes32) {
        // At block N, we get the beacon block root for block N - 1. So, to get the block root of the Nth block,
        // we query the root at block N + 1. If N + 1 is a missed slot, we keep querying until we find a block N + x
        // that has the block root for Nth block.
        uint256 targetTimestamp = timestamp + PreconfConstants.SECONDS_IN_SLOT;
        while (true) {
            (bool success, bytes memory result) = beaconBlockRootContract.staticcall(abi.encode(targetTimestamp));
            if (success && result.length > 0) {
                return abi.decode(result, (bytes32));
            }

            unchecked {
                targetTimestamp += PreconfConstants.SECONDS_IN_SLOT;
            }
        }
        return bytes32(0);
    }

    //=======
    // Views
    //=======

    /// @dev We use the beacon block root at the first block in the last epoch as randomness to
    ///  decide on the preconfer for the given epoch
    function getFallbackPreconfer(uint256 epochTimestamp) public view returns (address) {
        // Start of the last epoch
        uint256 lastEpochTimestamp = epochTimestamp - PreconfConstants.SECONDS_IN_EPOCH;
        uint256 randomness = uint256(_getBeaconBlockRoot(lastEpochTimestamp));
        uint256 preconferIndex = randomness % preconfRegistry.getNextPreconferIndex();

        if (preconferIndex == 0) {
            preconferIndex = 1;
        }

        return preconfRegistry.getPreconferAtIndex(preconferIndex);
    }

    /**
     * @notice Returns the full 32 slot preconfer lookahead for the epoch
     * @dev This function has been added as a helper for the node to get the full 32 slot lookahead without
     * the need of deconstructing the contract storage. Due to the fact that we are deconstructing an efficient
     * data structure to fill in all the slots, this is very heavy on gas, and onchain calls to it should be avoided.
     * @param epochTimestamp The start timestamp of the epoch for which the lookahead is to be generated
     */
    function getLookaheadForEpoch(uint256 epochTimestamp) external view returns (address[SLOTS_IN_EPOCH] memory) {
        address[SLOTS_IN_EPOCH] memory lookaheadForEpoch;

        uint256 _lookaheadTail = lookaheadTail;
        uint256 lastSlotTimestamp =
            epochTimestamp + PreconfConstants.SECONDS_IN_EPOCH - PreconfConstants.SECONDS_IN_SLOT;

        // Take the tail to the entry that fills the last slot of the epoch.
        // This may be an entry in the next epoch who starts preconfing in advanced.
        // This may also be an empty slot since the lookahead for next epoch is not yet posted.
        while (lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].prevTimestamp >= lastSlotTimestamp) {
            _lookaheadTail -= 1;
        }

        address preconfer = lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].preconfer;
        uint256 prevTimestamp = lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].prevTimestamp;
        uint256 timestamp = uint256(lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].timestamp);

        // Iterate backwards and fill in the slots
        for (uint256 i = SLOTS_IN_EPOCH; i > 0; --i) {
            if (timestamp >= lastSlotTimestamp) {
                lookaheadForEpoch[i - 1] = preconfer;
            }

            lastSlotTimestamp -= PreconfConstants.SECONDS_IN_SLOT;
            if (lastSlotTimestamp == prevTimestamp) {
                _lookaheadTail -= 1;
                preconfer = lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].preconfer;
                prevTimestamp = lookahead[_lookaheadTail % LOOKAHEAD_BUFFER_SIZE].prevTimestamp;
            }
        }

        return lookaheadForEpoch;
    }

    /**
     * @notice Builds and returns lookahead set parameters for an epoch
     * @dev This function can be used by the offchain node to create the lookahead to be posted.
     * @param epochTimestamp The start timestamp of the epoch for which the lookahead is to be generated
     * @param validatorBLSPubKeys The BLS public keys of the validators who are expected to propose in the epoch
     * in the same sequence as they appear in the epoch. So at index n - 1, we have the validator for slot n in that
     * epoch.
     */
    function getLookaheadParamsForEpoch(uint256 epochTimestamp, bytes[SLOTS_IN_EPOCH] memory validatorBLSPubKeys)
        external
        view
        returns (LookaheadSetParam[] memory)
    {
        uint256 index;
        LookaheadSetParam[32] memory lookaheadSetParamsTemp;

        for (uint256 i = 0; i < 32; ++i) {
            uint256 slotTimestamp = epochTimestamp + (i * PreconfConstants.SECONDS_IN_SLOT);

            // Fetch the validator object from the registry
            IPreconfRegistry.Validator memory validator =
                preconfRegistry.getValidator(keccak256(abi.encodePacked(bytes16(0), validatorBLSPubKeys[i])));

            // Skip deregistered preconfers
            if (preconfRegistry.getPreconferIndex(validator.preconfer) == 0) {
                continue;
            }

            // If the validator is allowed to propose in the epoch, add the associated preconfer to the lookahead
            if (
                validator.preconfer != address(0) && slotTimestamp >= validator.startProposingAt
                    && (validator.stopProposingAt == 0 || slotTimestamp < validator.stopProposingAt)
            ) {
                lookaheadSetParamsTemp[index] =
                    LookaheadSetParam({timestamp: slotTimestamp, preconfer: validator.preconfer});
                ++index;
            }
        }

        // Not very gas efficient, but is okay for a view expected to be used offchain
        LookaheadSetParam[] memory lookaheadSetParams = new LookaheadSetParam[](index);
        for (uint256 i; i < index; ++i) {
            lookaheadSetParams[i] = lookaheadSetParamsTemp[i];
        }

        return lookaheadSetParams;
    }

    function isLookaheadRequired(uint256 epochTimestamp) public view returns (bool) {
        return lookaheadPosters[epochTimestamp] == address(0);
    }

    function getLookaheadTail() external view returns (uint256) {
        return lookaheadTail;
    }

    function getLookaheadBuffer() external view returns (LookaheadBufferEntry[LOOKAHEAD_BUFFER_SIZE] memory) {
        return lookahead;
    }

    function getLookaheadPoster(uint256 epochTimestamp) external view returns (address) {
        return lookaheadPosters[epochTimestamp];
    }

    function getBlockProposer(uint256 blockId) external view returns (address) {
        return blockIdToProposer[blockId];
    }
}
