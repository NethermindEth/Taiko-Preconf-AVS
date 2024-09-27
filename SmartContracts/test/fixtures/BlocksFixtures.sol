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
import {IPreconfTaskManager} from "src/interfaces/IPreconfTaskManager.sol";
import {ITaikoL1} from "src/interfaces/taiko/ITaikoL1.sol";

contract BlocksFixtures is BaseTest {
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

    /// @dev Inserts two preconfers in the lookahead for the next epoch at the given slots.
    function prepareLookahead(uint256 slot1, uint256 slot2) internal {
        addPreconfersToRegistry(3);

        uint256 nextEpochStart = PreconfConstants.MAINNET_BEACON_GENESIS + PreconfConstants.SECONDS_IN_EPOCH;

        IPreconfTaskManager.LookaheadSetParam[] memory lookaheadSetParams =
            new IPreconfTaskManager.LookaheadSetParam[](2);
        lookaheadSetParams[0] = IPreconfTaskManager.LookaheadSetParam({
            preconfer: addr_1,
            timestamp: nextEpochStart + PreconfConstants.SECONDS_IN_SLOT * (slot1 - 1)
        });
        lookaheadSetParams[1] = IPreconfTaskManager.LookaheadSetParam({
            preconfer: addr_3,
            timestamp: nextEpochStart + PreconfConstants.SECONDS_IN_SLOT * (slot2 - 1)
        });

        vm.warp(PreconfConstants.MAINNET_BEACON_GENESIS);
        vm.prank(addr_1);
        preconfTaskManager.forcePushLookahead(lookaheadSetParams);
    }

    function addPreconfersToRegistry(uint256 count) internal {
        for (uint256 i = 1; i <= count; i++) {
            preconfRegistry.registerPreconfer(vm.addr(i));
        }
    }

    function setupTaikoBlock(uint256 id, uint256 proposedAt, bytes32 txListHash)
        internal
        returns (ITaikoL1.BlockMetadataV2 memory)
    {
        ITaikoL1.BlockMetadataV2 memory metadata;

        metadata.blobHash = txListHash;
        metadata.id = uint64(id);

        taikoL1.setBlock(keccak256(abi.encode(metadata)), proposedAt);

        return metadata;
    }
}
