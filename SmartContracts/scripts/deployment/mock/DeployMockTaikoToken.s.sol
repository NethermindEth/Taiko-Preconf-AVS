// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BaseScript} from "../../BaseScript.sol";

import {MockTaikoToken} from "src/mock/MockTaikoToken.sol";

import {console2} from "forge-std/Script.sol";

contract DeployMockTaikoToken is BaseScript {
    function run() external broadcast {
        MockTaikoToken myContract = new MockTaikoToken();
        console2.log("MockTaikoToken:", address(myContract));
    }
}
