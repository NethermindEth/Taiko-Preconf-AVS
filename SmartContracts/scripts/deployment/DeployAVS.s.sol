// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BaseScript} from "../BaseScript.sol";
import {EmptyContract} from "../misc/EmptyContract.sol";

import {PreconfRegistry} from "src/avs/PreconfRegistry.sol";
import {PreconfServiceManager} from "src/avs/PreconfServiceManager.sol";
import {PreconfTaskManager} from "src/avs/PreconfTaskManager.sol";
import {IPreconfRegistry} from "src/interfaces/IPreconfRegistry.sol";
import {IPreconfServiceManager} from "src/interfaces/IPreconfServiceManager.sol";
import {IPreconfTaskManager} from "src/interfaces/IPreconfTaskManager.sol";
import {IAVSDirectory} from "src/interfaces/eigenlayer-mvp/IAVSDirectory.sol";
import {ISlasher} from "src/interfaces/eigenlayer-mvp/ISlasher.sol";
import {ITaikoL1} from "src/interfaces/taiko/ITaikoL1.sol";

import {console2} from "forge-std/Script.sol";
import {ProxyAdmin} from "openzeppelin-contracts/proxy/transparent/ProxyAdmin.sol";
import {ITransparentUpgradeableProxy} from "openzeppelin-contracts/proxy/transparent/TransparentUpgradeableProxy.sol";

contract DeployAVS is BaseScript {
    // Required by service manager
    address public avsDirectory = vm.envAddress("AVS_DIRECTORY");
    address public slasher = vm.envAddress("SLASHER");

    // Required by task manager
    address public taikoL1 = vm.envAddress("TAIKO_L1");
    uint256 public beaconGenesisTimestamp = vm.envUint("BEACON_GENESIS_TIMESTAMP");
    address public beaconBlockRootContract = vm.envAddress("BEACON_BLOCK_ROOT_CONTRACT");

    function run() external broadcast {
        EmptyContract emptyContract = new EmptyContract();
        ProxyAdmin proxyAdmin = new ProxyAdmin();

        // Deploy proxies with empty implementations
        address preconfRegistry = deployProxy(address(emptyContract), address(proxyAdmin), "");
        address preconfServiceManager = deployProxy(address(emptyContract), address(proxyAdmin), "");
        address preconfTaskManager = deployProxy(address(emptyContract), address(proxyAdmin), "");

        // Deploy implementations
        PreconfRegistry preconfRegistryImpl = new PreconfRegistry(IPreconfServiceManager(preconfServiceManager));
        PreconfServiceManager preconfServiceManagerImpl = new PreconfServiceManager(
            preconfRegistry, preconfTaskManager, IAVSDirectory(avsDirectory), ISlasher(slasher)
        );
        PreconfTaskManager preconfTaskManagerImpl = new PreconfTaskManager(
            IPreconfServiceManager(preconfServiceManager),
            IPreconfRegistry(preconfRegistry),
            ITaikoL1(taikoL1),
            beaconGenesisTimestamp,
            beaconBlockRootContract
        );

        // Upgrade proxies with implementations
        proxyAdmin.upgrade(ITransparentUpgradeableProxy(preconfRegistry), address(preconfRegistryImpl));
        proxyAdmin.upgrade(ITransparentUpgradeableProxy(preconfServiceManager), address(preconfServiceManagerImpl));
        proxyAdmin.upgrade(ITransparentUpgradeableProxy(preconfTaskManager), address(preconfTaskManagerImpl));

        console2.log("Proxy admin: ", address(proxyAdmin));
        console2.log("Preconf Registry: ", preconfRegistry);
        console2.log("Preconf Service Manager: ", preconfServiceManager);
        console2.log("Preconf Task Manager: ", preconfTaskManager);
    }
}