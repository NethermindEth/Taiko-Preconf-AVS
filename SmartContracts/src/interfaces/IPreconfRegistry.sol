// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BLS12381} from "../libraries/BLS12381.sol";
import {ISignatureUtils} from "eigenlayer-middleware/interfaces/IServiceManagerUI.sol";

interface IPreconfRegistry {
    event PreconferRegistered(address indexed preconfer, uint256 indexed index);
    event PreconferDeregistered(address indexed preconfer);
    event ValidatorAdded(address indexed preconfer, uint256[2] compressedPubKey);
    event ValidatorRemoved(address indexed preconfer, bytes32 validatorPubKeyHash);

    /// @dev The preconfer is already registered in the registry
    error PreconferAlreadyRegistered();
    /// @dev The preconfer is not registered in the registry
    error PreconferNotRegistered();
    /// @dev The length of the public keys and signatures arrays do not match
    error ArrayLengthMismatch();
    /// @dev The signature is invalid
    error InvalidValidatorSignature();
    /// @dev The address provided as witness of the preconfer that has the last index is incorrect
    error LastIndexWitnessIncorrect();
    /// @dev The public key hash is not associated with the preconfer
    error InvalidValidatorPubKeyHash();

    /// @dev Registers a preconfer by giving them a non-zero registry index
    function registerPreconfer(ISignatureUtils.SignatureWithSaltAndExpiry calldata operatorSignature) external;

    /// @dev Deregisters a preconfer from the registry
    function deregisterPreconfer(address lastIndexWitness) external;

    /// @dev Associates a batch of validators with a preconfer
    function addValidators(BLS12381.G1Point[] calldata pubkeys, BLS12381.G2Point[] calldata signatures) external;

    /// @dev Removes a batch of validators for a preconfer
    function removeValidators(bytes32[] calldata validatorPubKeyHashes) external;

    /// @dev Returns the index of the preconfer
    function preconferToIndex(address preconfer) external view returns (uint256);

    /// @dev Returns the nonce of the preconfer
    function preconferToNonce(address preconfer) external view returns (uint256);

    /// @dev Returns the timestamp at which the public key hash was added
    function preconferToPubKeyHashToTimestamp(address preconfer, bytes32 pubKeyHash) external view returns (uint256);
}
