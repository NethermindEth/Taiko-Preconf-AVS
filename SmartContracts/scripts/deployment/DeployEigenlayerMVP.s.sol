// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BaseScript} from "../BaseScript.sol";
import {EmptyContract} from "../misc/EmptyContract.sol";

import {AVSDirectory} from "src/eigenlayer-mvp/AVSDirectory.sol";
import {DelegationManager} from "src/eigenlayer-mvp/DelegationManager.sol";
import {StrategyManager} from "src/eigenlayer-mvp/StrategyManager.sol";
import {Slasher} from "src/eigenlayer-mvp/Slasher.sol";
import {IDelegationManager} from "src/interfaces/eigenlayer-mvp/IDelegationManager.sol";
import {IStrategyManager} from "src/interfaces/eigenlayer-mvp/IStrategyManager.sol";

import {console2} from "forge-std/Script.sol";
import {ProxyAdmin} from "openzeppelin-contracts/proxy/transparent/ProxyAdmin.sol";
import {ITransparentUpgradeableProxy} from "openzeppelin-contracts/proxy/transparent/TransparentUpgradeableProxy.sol";

contract DeployEigenlayerMVP is BaseScript {
    function run() external broadcast {
        EmptyContract emptyContract = new EmptyContract();
        ProxyAdmin proxyAdmin = new ProxyAdmin();

        // Deploy proxies with empty implementations
        address avsDirectory = deployProxy(address(emptyContract), address(proxyAdmin), "");
        address delegationManager = deployProxy(address(emptyContract), address(proxyAdmin), "");
        address strategyManager = deployProxy(address(emptyContract), address(proxyAdmin), "");
        address slasher = deployProxy(address(emptyContract), address(proxyAdmin), "");

        // Deploy implementations
        AVSDirectory avsDirectoryImpl = new AVSDirectory();
        DelegationManager delegationManagerImpl = new DelegationManager(IStrategyManager(strategyManager));
        StrategyManager strategyManagerImpl = new StrategyManager(IDelegationManager(delegationManager));
        Slasher slasherImpl = new Slasher();

        // Log the addresses of the implementations
        console2.log("AVSDirectory Implementation: ", address(avsDirectoryImpl));
        console2.log("DelegationManager Implementation: ", address(delegationManagerImpl));
        console2.log("StrategyManager Implementation: ", address(strategyManagerImpl));
        console2.log("Slasher Implementation: ", address(slasherImpl));

  // Upgrade proxies with implementations
        try proxyAdmin.upgrade(ITransparentUpgradeableProxy(avsDirectory), address(avsDirectoryImpl)) {
            console2.log("AVS Directory upgraded successfully");
        } catch (bytes memory reason) {
            console2.log("AVS Directory upgrade failed: ", string(reason));
        }

        try proxyAdmin.upgrade(ITransparentUpgradeableProxy(delegationManager), address(delegationManagerImpl)) {
            console2.log("Delegation Manager upgraded successfully");
        } catch (bytes memory reason) {
            console2.log("Delegation Manager upgrade failed: ", string(reason));
        }

        try proxyAdmin.upgrade(ITransparentUpgradeableProxy(strategyManager), address(strategyManagerImpl)) {
            console2.log("Strategy Manager upgraded successfully");
        } catch (bytes memory reason) {
            console2.log("Strategy Manager upgrade failed: ", string(reason));
        }

        try proxyAdmin.upgrade(ITransparentUpgradeableProxy(slasher), address(slasherImpl)) {
            console2.log("Slasher upgraded successfully");
        } catch (bytes memory reason) {
            console2.log("Slasher upgrade failed: ", string(reason));
        }

        console2.log("AVS Directory: ", avsDirectory);
        console2.log("Delegation Manager: ", delegationManager);
        console2.log("Strategy Manager: ", strategyManager);
        console2.log("Slasher: ", slasher);
    }

    function isContract(address addr) internal view returns (bool) {
        uint256 size;
        assembly {
            size := extcodesize(addr)
        }
        return size > 0;
    }
}
