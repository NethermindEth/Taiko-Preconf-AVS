// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

interface ITaikoL1 {
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

    struct EthDeposit {
        address recipient;
        uint96 amount;
        uint64 id;
    }

    struct SlotA {
        uint64 genesisHeight;
        uint64 genesisTimestamp;
        uint64 lastSyncedBlockId;
        uint64 lastSynecdAt; // typo!
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

    struct BlockV2 {
        bytes32 metaHash; // slot 1
        address assignedProver; // slot 2
        uint96 livenessBond;
        uint64 blockId; // slot 3
        // Before the fork, this field is the L1 timestamp when this block is proposed.
        // After the fork, this is the timestamp of the L2 block.
        // In a later fork, we an rename this field to `timestamp`.
        uint64 proposedAt;
        // Before the fork, this field is the L1 block number where this block is proposed.
        // After the fork, this is the L1 block number input for the anchor transaction.
        // In a later fork, we an rename this field to `anchorBlockId`.
        uint64 proposedIn;
        uint24 nextTransitionId;
        bool livenessBondReturned;
        // The ID of the transaction that is used to verify this block. However, if
        // this block is not verified as the last block in a batch, verifiedTransitionId
        // will remain zero.
        uint24 verifiedTransitionId;
    }

    function proposeBlockV2(
        bytes calldata _params,
        bytes calldata _txList
    )
        external
        payable
        returns (BlockMetadataV2 memory meta_);

     function proposeBlocksV2(
        bytes[] calldata _paramsArr,
        bytes[] calldata _txListArr
    )
        external
        payable
        returns (BlockMetadataV2[] memory metaArr_);

    function getStateVariables() external view returns (SlotA memory, SlotB memory);

    function getBlockV2(uint64 _blockId) external view returns (BlockV2 memory blk_);
}
