// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ITaikoL1} from "src/interfaces/taiko/ITaikoL1.sol";

contract MockTaikoL1 is ITaikoL1 {
    bytes public params;
    bytes public txList;
    uint256 public blockId;

    function proposeBlock(bytes calldata _params, bytes calldata _txList)
        external
        payable
        returns (BlockMetadata memory a, EthDeposit[] memory b)
    {
        params = _params;
        txList = _txList;

        return (a, b);
    }

    function getStateVariables() external view returns (SlotA memory a, SlotB memory b) {
        b.numBlocks = uint64(blockId);
        return (a, b);
    }

    function getBlock(uint64 _blockId) external view returns (Block memory blk_) {}

    /// @dev Force set for testing
    function setBlockId(uint256 id) external {
        blockId = id;
    }
}
