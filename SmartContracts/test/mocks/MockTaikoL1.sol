// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ITaikoL1} from "src/interfaces/taiko/ITaikoL1.sol";

contract MockTaikoL1 is ITaikoL1 {
    function proposeBlock(bytes calldata _params, bytes calldata _txList)
        external
        payable
        returns (BlockMetadata memory meta_, EthDeposit[] memory deposits_)
    {}

    function getStateVariables() external view returns (SlotA memory, SlotB memory) {}

    function getBlock(uint64 _blockId) external view returns (Block memory blk_) {}
}
