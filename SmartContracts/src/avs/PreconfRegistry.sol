// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BLS12381} from "../libraries/BLS12381.sol";
import {BLSSignatureChecker} from "./utils/BLSSignatureChecker.sol";
import {IPreconfRegistry} from "../interfaces/IPreconfRegistry.sol";
import {IServiceManager} from "eigenlayer-middleware/interfaces/IServiceManager.sol";
import {ISignatureUtils} from "eigenlayer-middleware/interfaces/IServiceManagerUI.sol";

contract PreconfRegistry is IPreconfRegistry, ISignatureUtils, BLSSignatureChecker {
    using BLS12381 for BLS12381.G1Point;

    IServiceManager internal immutable preconfServiceManager;

    uint256 internal nextPreconferIndex;

    // Maps the preconfer's address to an index that may change over the lifetime of a preconfer
    mapping(address => uint256) public preconferToIndex;

    // Maps the preconfer's address to an incrementing nonce used for validator signatures
    mapping(address => uint256) public preconferToNonce;

    // Maps the preconfer's ecsda and one associated BLS public key hash to the timestamp
    // at which the key hash was added
    mapping(address => mapping(bytes32 => uint256)) public preconferToPubKeyHashToTimestamp;

    constructor(IServiceManager _preconfServiceManager) {
        preconfServiceManager = _preconfServiceManager;
        nextPreconferIndex = 1;
    }

    /**
     * @notice Registers a preconfer by giving them a non-zero registry index
     * @param operatorSignature The signature of the preconfer in the format expected by Eigenlayer registry
     */
    function registerPreconfer(SignatureWithSaltAndExpiry calldata operatorSignature) external {
        // Preconfer must not have registered already
        if (preconferToIndex[msg.sender] != 0) {
            revert PreconferAlreadyRegistered();
        }

        uint256 _nextPreconferIndex = nextPreconferIndex;

        preconferToIndex[msg.sender] = _nextPreconferIndex;
        nextPreconferIndex = _nextPreconferIndex + 1;

        emit PreconferRegistered(msg.sender, _nextPreconferIndex);

        preconfServiceManager.registerOperatorToAVS(msg.sender, operatorSignature);
    }

    /**
     * @notice Deregisters a preconfer from the registry
     * @dev The preconfer that has the last index must be provided as a witness to save gas
     * @param lastIndexWitness The address of the preconfer that has the last index
     */
    function deregisterPreconfer(address lastIndexWitness) external {
        // Preconfer must have registered already
        if (preconferToIndex[msg.sender] == 0) {
            revert PreconferNotRegistered();
        }

        // Ensure that provided witness is the preconfer that has the last index
        uint256 _nextPreconferIndex = nextPreconferIndex - 1;
        if (preconferToIndex[lastIndexWitness] != _nextPreconferIndex) {
            revert LastIndexWitnessIncorrect();
        }

        // Update to the decremented index to account for the removed preconfer
        nextPreconferIndex = _nextPreconferIndex;

        // Remove the preconfer and exchange its index with the last preconfer
        uint256 removedPreconferIndex = preconferToIndex[msg.sender];
        preconferToIndex[msg.sender] = 0;
        preconferToIndex[lastIndexWitness] = removedPreconferIndex;

        emit PreconferDeregistered(msg.sender);

        preconfServiceManager.deregisterOperatorFromAVS(msg.sender);
    }

    /**
     * @notice Associates a batch of validators with a preconfer
     * @param pubkeys The public keys of the validators
     * @param signatures The BLS signatures of the validators
     */
    function addValidators(BLS12381.G1Point[] calldata pubkeys, BLS12381.G2Point[] calldata signatures) external {
        if (pubkeys.length != signatures.length) {
            revert ArrayLengthMismatch();
        }

        uint256 preconferNonce = preconferToNonce[msg.sender];
        for (uint256 i; i < pubkeys.length; ++i) {
            // Revert if any signature is invalid
            if (!verifySignature(_createMessage(preconferNonce), signatures[i], pubkeys[i])) {
                revert InvalidValidatorSignature();
            }

            // Point compress the public key just how it is done on the consensus layer
            uint256[2] memory compressedPubKey = pubkeys[i].compress();
            // Use the hash for ease of mapping
            bytes32 pubKeyHash = keccak256(abi.encodePacked(compressedPubKey));

            preconferToPubKeyHashToTimestamp[msg.sender][pubKeyHash] = block.timestamp;

            emit ValidatorAdded(msg.sender, compressedPubKey);

            unchecked {
                ++preconferNonce;
            }
        }

        preconferToNonce[msg.sender] = preconferNonce;
    }

    /**
     * @notice Removes a batch of validators for a preconfer
     * @param validatorPubKeyHashes The hashes of the public keys of the validators
     */
    function removeValidators(bytes32[] memory validatorPubKeyHashes) external {
        for (uint256 i; i < validatorPubKeyHashes.length; ++i) {
            if (preconferToPubKeyHashToTimestamp[msg.sender][validatorPubKeyHashes[i]] == 0) {
                revert InvalidValidatorPubKeyHash();
            }
            preconferToPubKeyHashToTimestamp[msg.sender][validatorPubKeyHashes[i]] = 0;
            emit ValidatorRemoved(msg.sender, validatorPubKeyHashes[i]);
        }
    }

    //=========
    // Helpers
    //=========

    /**
     * @notice Returns the message to be signed by the preconfer
     * @param nonce The nonce of the preconfer
     */
    function _createMessage(uint256 nonce) internal view returns (bytes memory) {
        return abi.encodePacked(block.chainid, msg.sender, nonce);
    }
}
