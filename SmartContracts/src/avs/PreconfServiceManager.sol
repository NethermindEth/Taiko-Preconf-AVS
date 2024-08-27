// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {IPreconfServiceManager} from "../interfaces/IPreconfServiceManager.sol";
import {ISlasher} from "../interfaces/eigenlayer-mvp/ISlasher.sol";
import {IAVSDirectory} from "../interfaces/eigenlayer-mvp/IAVSDirectory.sol";

contract PreconfServiceManager is IPreconfServiceManager {
    address internal immutable preconfRegistry;
    address internal immutable preconfTaskManager;
    IAVSDirectory internal immutable avsDirectory;
    ISlasher internal immutable slasher;

    mapping(address operator => uint256 timestamp) public stakeLockedUntil;

    constructor(address _preconfRegistry, address _preconfTaskManager, IAVSDirectory _avsDirectory, ISlasher _slasher) {
        _preconfRegistry;
        preconfTaskManager = _preconfTaskManager;
        avsDirectory = _avsDirectory;
        slasher = _slasher;
    }

    modifier onlyPreconfTaskManager() {
        if (msg.sender != address(preconfTaskManager)) {
            revert SenderIsNotPreconfTaskManager();
        }
        _;
    }

    modifier onlyPreconfRegistry() {
        if (msg.sender != preconfRegistry) {
            revert SenderIsNotPreconfRegistry();
        }
        _;
    }

    /// @dev Simply relays the call to the AVS directory
    function registerOperatorToAVS(address operator, IAVSDirectory.SignatureWithSaltAndExpiry memory operatorSignature)
        external
        onlyPreconfRegistry
    {
        avsDirectory.registerOperatorToAVS(operator, operatorSignature);
    }

    /// @dev Simply relays the call to the AVS directory
    function deregisterOperatorFromAVS(address operator) external onlyPreconfRegistry {
        avsDirectory.deregisterOperatorFromAVS(operator);
    }

    /// @dev This not completely functional until Eigenlayer decides the logic of their Slasher.
    ///  for now this simply sets a value in the storage and releases an event.
    function lockStakeUntil(address operator, uint256 timestamp) external onlyPreconfTaskManager {
        stakeLockedUntil[operator] = timestamp;
        emit StakeLockedUntil(operator, timestamp);
    }

    /// @dev This not completely functional until Eigenlayer decides the logic of their Slasher.
    function slashOperator(address operator) external onlyPreconfTaskManager {
        if (slasher.isOperatorSlashed(operator)) {
            revert OperatorAlreadySlashed();
        }
        slasher.slashOperator(operator);
    }
}
