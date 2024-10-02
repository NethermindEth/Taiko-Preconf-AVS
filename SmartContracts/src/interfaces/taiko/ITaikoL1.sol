// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

/// @dev Manual copy of lib/taiko-mono/packages/protocol/contracts/L1/TaikoL1.sol and its dependent structs
/// from https://github.com/taikoxyz/taiko-mono/tree/protocol-v1.9.0.
interface ITaikoL1 {
    /// @dev Struct that represents L2 basefee configurations
    struct BaseFeeConfig {
        uint8 adjustmentQuotient;
        uint8 sharingPctg;
        uint32 gasIssuancePerSecond;
        uint64 minGasExcess;
        uint32 maxGasIssuancePerBlock;
    }

    struct BlockMetadataV2 {
        bytes32 anchorBlockHash; // `_l1BlockHash` in TaikoL2's anchor tx.
        bytes32 difficulty;
        bytes32 blobHash;
        bytes32 extraData;
        address coinbase;
        uint64 id;
        uint32 gasLimit;
        uint64 timestamp;
        uint64 anchorBlockId; // `_l1BlockId` in TaikoL2's anchor tx.
        uint16 minTier;
        bool blobUsed;
        bytes32 parentMetaHash;
        address proposer;
        uint96 livenessBond;
        // Time this block is proposed at, used to check proving window and cooldown window.
        uint64 proposedAt;
        // L1 block number, required/used by node/client.
        uint64 proposedIn;
        uint32 blobTxListOffset;
        uint32 blobTxListLength;
        uint8 blobIndex;
        BaseFeeConfig baseFeeConfig;
    }

    struct SlotA {
        uint64 genesisHeight;
        uint64 genesisTimestamp;
        uint64 lastSyncedBlockId;
        uint64 lastSynecdAt; // known typo (lastSyncedAt)
    }

    struct SlotB {
        uint64 numBlocks;
        uint64 lastVerifiedBlockId;
        bool provingPaused;
        uint8 __reservedB1;
        uint16 __reservedB2;
        uint32 __reservedB3;
        uint64 lastUnpausedAt;
    }

    /// @dev Struct containing data required for verifying a block.
    /// 3 slots used.
    struct BlockV2 {
        bytes32 metaHash; // slot 1
        address assignedProver; // slot 2
        uint96 livenessBond;
        uint64 blockId; // slot 3
        // Before the fork, this field is the L1 timestamp when this block is proposed.
        // After the fork, this is the timestamp of the L2 block.
        // In a later fork, we an rename this field to `timestamp`.
        uint64 proposedAt;
        // This is the L1 block number input for the anchor transaction.
        // In a later fork, we can rename this field to `anchorBlockId`.
        uint64 proposedIn;
        uint24 nextTransitionId;
        bool livenessBondReturned;
        // The ID of the transaction that is used to verify this block. However, if this block is
        // not verified as the last block in a batch, verifiedTransitionId will remain zero.
        uint24 verifiedTransitionId;
    }

    /// @notice Proposes a Taiko L2 block (version 2)
    /// @param _params Block parameters, an encoded BlockParamsV2 object.
    /// @param _txList txList data if calldata is used for DA.
    /// @return meta_ The metadata of the proposed L2 block.
    function proposeBlockV2(bytes calldata _params, bytes calldata _txList)
        external
        returns (BlockMetadataV2 memory meta_);

    /// @notice Proposes multiple Taiko L2 blocks (version 2).
    /// @param _paramsArr List of encoded BlockParamsV2 objects.
    /// @param _txListArr List of transaction lists.
    /// @return metaArr_ Metadata objects of the proposed L2 blocks.
    function proposeBlocksV2(bytes[] calldata _paramsArr, bytes[] calldata _txListArr)
        external
        returns (BlockMetadataV2[] memory metaArr_);

    /// @notice Gets the state variables of the TaikoL1 contract.
    /// @dev This method can be deleted once node/client stops using it.
    /// @return State variables stored at SlotA.
    /// @return State variables stored at SlotB.
    function getStateVariables() external view returns (SlotA memory, SlotB memory);

    /// @notice Gets the details of a block.
    /// @param _blockId Index of the block.
    /// @return blk_ The block.
    function getBlockV2(uint64 _blockId) external view returns (BlockV2 memory blk_);
}
