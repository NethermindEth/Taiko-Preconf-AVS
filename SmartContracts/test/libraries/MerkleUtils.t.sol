// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {MerkleUtils} from "src/libraries/MerkleUtils.sol";
import {BaseTest} from "../BaseTest.sol";

contract TestMerkleUtils is BaseTest {
    using MerkleUtils for bytes32[8];
    using MerkleUtils for bytes32;

    function test_merkleize_and_verify_chunks() external pure {
        bytes32[8] memory chunks;

        chunks[0] = sha256("chunk0");
        chunks[1] = sha256("chunk1");
        chunks[2] = sha256("chunk2");
        chunks[3] = sha256("chunk3");
        chunks[4] = sha256("chunk4");
        chunks[5] = sha256("chunk5");
        chunks[6] = sha256("chunk6");
        chunks[7] = sha256("chunk7");

        bytes32 expectedRoot = sha256(
            abi.encodePacked(
                sha256(abi.encodePacked(chunks[0].hash(chunks[1]), chunks[2].hash(chunks[3]))),
                sha256(abi.encodePacked(chunks[4].hash(chunks[5]), chunks[6].hash(chunks[7])))
            )
        );
        assertEq(chunks.merkleize(), expectedRoot);

        bytes32[] memory proof = new bytes32[](3);
        proof[0] = chunks[1];
        proof[1] = chunks[2].hash(chunks[3]);
        proof[2] = sha256(abi.encodePacked(chunks[4].hash(chunks[5]), chunks[6].hash(chunks[7])));

        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[0], 0));

        proof[0] = chunks[0];
        proof[1] = chunks[2].hash(chunks[3]);
        proof[2] = sha256(abi.encodePacked(chunks[4].hash(chunks[5]), chunks[6].hash(chunks[7])));
        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[1], 1));

        proof[0] = chunks[3];
        proof[1] = chunks[0].hash(chunks[1]);
        proof[2] = sha256(abi.encodePacked(chunks[4].hash(chunks[5]), chunks[6].hash(chunks[7])));
        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[2], 2));

        proof[0] = chunks[2];
        proof[1] = chunks[0].hash(chunks[1]);
        proof[2] = sha256(abi.encodePacked(chunks[4].hash(chunks[5]), chunks[6].hash(chunks[7])));
        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[3], 3));

        proof[0] = chunks[5];
        proof[1] = chunks[6].hash(chunks[7]);
        proof[2] = sha256(abi.encodePacked(chunks[0].hash(chunks[1]), chunks[2].hash(chunks[3])));
        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[4], 4));

        proof[0] = chunks[4];
        proof[1] = chunks[6].hash(chunks[7]);
        proof[2] = sha256(abi.encodePacked(chunks[0].hash(chunks[1]), chunks[2].hash(chunks[3])));
        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[5], 5));

        proof[0] = chunks[7];
        proof[1] = chunks[4].hash(chunks[5]);
        proof[2] = sha256(abi.encodePacked(chunks[0].hash(chunks[1]), chunks[2].hash(chunks[3])));
        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[6], 6));

        proof[0] = chunks[6];
        proof[1] = chunks[4].hash(chunks[5]);
        proof[2] = sha256(abi.encodePacked(chunks[0].hash(chunks[1]), chunks[2].hash(chunks[3])));
        assertTrue(MerkleUtils.verifyProof(proof, expectedRoot, chunks[7], 7));
    }
}
