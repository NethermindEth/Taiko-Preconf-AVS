// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BaseTest} from "../BaseTest.sol";
import {MockPreconfRegistry} from "../mocks/MockPreconfRegistry.sol";
import {MockPreconfServiceManager} from "../mocks/MockPreconfServiceManager.sol";
import {MockBeaconBlockRoot} from "../mocks/MockBeaconBlockRoot.sol";
import {MockTaikoL1} from "../mocks/MockTaikoL1.sol";

import {PreconfConstants} from "src/avs/PreconfConstants.sol";
import {PreconfTaskManager} from "src/avs/PreconfTaskManager.sol";
import {IPreconfRegistry} from "src/interfaces/IPreconfRegistry.sol";
import {IPreconfServiceManager} from "src/interfaces/IPreconfServiceManager.sol";
import {ITaikoL1} from "src/interfaces/taiko/ITaikoL1.sol";

contract LookaheadFixtures is BaseTest {
    PreconfTaskManager internal preconfTaskManager;
    MockPreconfRegistry internal preconfRegistry;
    MockPreconfServiceManager internal preconfServiceManager;
    MockBeaconBlockRoot internal beaconBlockRootContract;
    MockTaikoL1 internal taikoL1;

    function setUp() public virtual {
        preconfRegistry = new MockPreconfRegistry();
        preconfServiceManager = new MockPreconfServiceManager();
        beaconBlockRootContract = new MockBeaconBlockRoot();
        taikoL1 = new MockTaikoL1();

        preconfTaskManager = new PreconfTaskManager(
            IPreconfServiceManager(address(preconfServiceManager)),
            IPreconfRegistry(address(preconfRegistry)),
            ITaikoL1(taikoL1),
            PreconfConstants.MAINNET_BEACON_GENESIS,
            address(beaconBlockRootContract)
        );
    }

    function addPreconfersToRegistry(uint256 count) internal {
        for (uint256 i = 1; i <= count; i++) {
            preconfRegistry.registerPreconfer(vm.addr(i));
        }
    }
}
