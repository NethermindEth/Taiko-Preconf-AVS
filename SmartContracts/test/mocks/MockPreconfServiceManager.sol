// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

contract MockPreconfServiceManager {
    mapping(address => uint256) public stakeLockTimestamps;
    mapping(address => bool) public operatorSlashed;

    function lockStakeUntil(address operator, uint256 timestamp) external {
        stakeLockTimestamps[operator] = timestamp;
    }

    function slashOperator(address operator) external {
        operatorSlashed[operator] = true;
    }
}
