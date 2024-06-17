// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {IPreconfServiceManager} from "../interfaces/IPreconfServiceManager.sol";
import {IPreconfTaskManager} from "../interfaces/IPreconfTaskManager.sol";
import {ISlasher} from "../interfaces/eigenlayer-mvp/ISlasher.sol";
import {
    IAVSDirectory,
    IRegistryCoordinator,
    IRewardsCoordinator,
    IStakeRegistry,
    ServiceManagerBase
} from "eigenlayer-middleware/ServiceManagerBase.sol";

contract PreconfServiceManager is ServiceManagerBase, IPreconfServiceManager {
    IPreconfTaskManager internal immutable preconfTaskManager;
    ISlasher internal immutable slasher;

    mapping(address operator => uint256 timestamp) public stakeLockedUntil;

    constructor(
        IAVSDirectory _avsDirectory,
        IRewardsCoordinator _rewardsCoordinator,
        IRegistryCoordinator _registryCoordinator,
        IStakeRegistry _stakeRegistry,
        IPreconfTaskManager _taskManager,
        ISlasher _slasher
    ) ServiceManagerBase(_avsDirectory, _rewardsCoordinator, _registryCoordinator, _stakeRegistry) {
        preconfTaskManager = _taskManager;
        slasher = _slasher;
    }

    modifier onlyPreconfTaskManager() {
        if (msg.sender != address(preconfTaskManager)) {
            revert IPreconfServiceManager.SenderIsNotPreconfTaskManager(msg.sender);
        }
        _;
    }

    function lockStakeUntil(address operator, uint256 timestamp) external onlyPreconfTaskManager {
        stakeLockedUntil[operator] = timestamp;
        emit StakeLockedUntil(operator, timestamp);
    }

    function slashOperator(address operator) external onlyPreconfTaskManager {
        if (slasher.isOperatorSlashed(operator)) {
            revert IPreconfServiceManager.OperatorAlreadySlashed(operator);
        }
        slasher.slashOperator(operator);
    }
}
