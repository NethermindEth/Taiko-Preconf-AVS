// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BaseTest} from "../BaseTest.sol";
import {BeaconProofs} from "../fixtures/BeaconProofs.sol";
import {MerkleUtils} from "src/libraries/MerkleUtils.sol";

/// @dev The beacon chain data used here is from slot 9000000 on Ethereum mainnet.
contract BeaconProofsVerification is BaseTest {
    function test_beaconProofsVerification_validatorInclusionInValidatorList() external pure {
        bytes32[8] memory validatorChunks = BeaconProofs.validatorChunks();

        bytes32 validatorHashTreeRoot = MerkleUtils.merkleize(validatorChunks);

        bytes32[] memory validatorProof = BeaconProofs.validatorProof();

        bytes32 validatorsRoot = BeaconProofs.validatorsRoot();
        uint256 validatorIndex = BeaconProofs.validatorIndex();

        vm.assertTrue(MerkleUtils.verifyProof(validatorProof, validatorsRoot, validatorHashTreeRoot, validatorIndex));
    }

    function test_beaconProofsVerification_validatorListInclusionInBeaconState() external pure {
        bytes32[] memory beaconStateProofForValidatorList = BeaconProofs.beaconStateProofForValidatorList();

        bytes32 validatorListRoot = BeaconProofs.validatorsRoot();
        bytes32 beaconStateRoot = BeaconProofs.beaconStateRoot();

        vm.assertTrue(MerkleUtils.verifyProof(beaconStateProofForValidatorList, beaconStateRoot, validatorListRoot, 11));
    }

    function test_beaconProofsVerification_beaconStateInclusionInBeaconBlock() external pure {
        bytes32[] memory beaconBlockProofForBeaconState = BeaconProofs.beaconBlockProofForBeaconState();

        bytes32 beaconStateRoot = BeaconProofs.beaconStateRoot();
        bytes32 beaconBlockRoot = BeaconProofs.beaconBlockRoot();

        vm.assertTrue(MerkleUtils.verifyProof(beaconBlockProofForBeaconState, beaconBlockRoot, beaconStateRoot, 3));
    }

    function test_beaconProofsVerification_proposerInclusionInBeaconBlock() external pure {
        bytes32[] memory beaconBlockProofForProposer = BeaconProofs.beaconBlockProofForProposer();

        uint256 validatorIndex = BeaconProofs.validatorIndex();
        bytes32 beaconBlockRoot = BeaconProofs.beaconBlockRoot();

        vm.assertTrue(
            MerkleUtils.verifyProof(
                beaconBlockProofForProposer, beaconBlockRoot, MerkleUtils.toLittleEndian(validatorIndex), 1
            )
        );
    }
}
