// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ITaikoL1} from "src/interfaces/taiko/ITaikoL1.sol";

contract MockTaikoL1 is ITaikoL1 {
    bytes public params;
    bytes public txList;
    uint256 public blockId;
    BlockV2 public blk;

    function proposeBlockV2(bytes calldata _params, bytes calldata _txList)
        external
        payable
        returns (BlockMetadataV2 memory meta_)
    {
        params = _params;
        txList = _txList;

        return meta_;
    }

    function proposeBlocksV2(bytes[] calldata _params, bytes[] calldata _txLists)
        external
        payable
        returns (BlockMetadataV2[] memory meta_)
    {
        params = _params[0];
        txList = _txLists[0];

        return meta_;
    }

    function getStateVariables() external view returns (SlotA memory a, SlotB memory b) {
        b.numBlocks = uint64(blockId);
        return (a, b);
    }

    function getBlockV2(uint64) external view returns (BlockV2 memory blk_) {
        return blk;
    }

    /// @dev Force set for testing
    function setBlockId(uint256 id) external {
        blockId = id;
    }

    /// @dev Force set for testing
    function setBlock(bytes32 metahash, uint256 proposedAt) external {
        blk.metaHash = metahash;
        blk.proposedAt = uint64(proposedAt);
    }
}
