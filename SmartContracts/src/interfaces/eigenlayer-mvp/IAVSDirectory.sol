// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

interface IAVSDirectory {
    struct SignatureWithSaltAndExpiry {
        // the signature itself, formatted as a single bytes object
        bytes signature;
        // the salt used to generate the signature
        bytes32 salt;
        // the expiration timestamp (UTC) of the signature
        uint256 expiry;
    }

    /// @dev This function will be left without implementation in the MVP
    function registerOperatorToAVS(address operator, SignatureWithSaltAndExpiry memory operatorSignature) external;

    /// @dev This function will be left without implementation in the MVP
    function deregisterOperatorFromAVS(address operator) external;
}
