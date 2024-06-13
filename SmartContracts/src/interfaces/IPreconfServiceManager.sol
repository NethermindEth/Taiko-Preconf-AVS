// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {ServiceManagerBase} from "eigenlayer-middleware/ServiceManagerBase.sol";

interface IPreconfServiceManager {
    event StakeLockedUntil(address indexed operator, uint256 timestamp);

    /// @dev Called by PreconfTaskManager to prevent withdrawals of stake during preconf or lookahead dispute period
    function lockStakeUntil(address operator, uint256 timestamp) external;

    /// @dev Called by PreconfTaskManager to slash an operator for incorret lookahead or preconfirmation
    function slashOperator(address operator) external;
}
