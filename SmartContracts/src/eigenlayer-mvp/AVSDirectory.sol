// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {IAVSDirectory} from "../interfaces/eigenlayer-mvp/IAVSDirectory.sol";

contract AVSDirectory is IAVSDirectory {
    function registerOperatorToAVS(address operator, IAVSDirectory.SignatureWithSaltAndExpiry memory operatorSignature)
        external
    {}

    function deregisterOperatorFromAVS(address operator) external {}
}
