// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {IAVSDirectory} from "./eigenlayer-mvp/IAVSDirectory.sol";

interface IPreconfServiceManager {
    event StakeLockedUntil(address indexed operator, uint256 timestamp);

    /// @dev Only callable by the task manager
    error SenderIsNotPreconfTaskManager();
    /// @dev Only callable by the registry
    error SenderIsNotPreconfRegistry();
    /// @dev The operator is already slashed
    error OperatorAlreadySlashed();

    /// @dev Only callable by the registry
    function registerOperatorToAVS(address operator, IAVSDirectory.SignatureWithSaltAndExpiry memory operatorSignature)
        external;

    /// @dev Only callable by the registry
    function deregisterOperatorFromAVS(address operator) external;

    /// @dev Called by PreconfTaskManager to prevent withdrawals of stake during preconf or lookahead dispute period
    function lockStakeUntil(address operator, uint256 timestamp) external;

    /// @dev Called by PreconfTaskManager to slash an operator for incorret lookahead or preconfirmation
    function slashOperator(address operator) external;
}
