// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {MerkleUtils} from "src/libraries/MerkleUtils.sol";
import {BaseTest} from "../BaseTest.sol";

contract TestMerkleUtils is BaseTest {
    using MerkleUtils for bytes32[8];

    function test_merkleize_and_verify_chunks() external pure {
        bytes32[8] memory chunks;

        chunks[0] = keccak256("chunk0");
        chunks[1] = keccak256("chunk1");
        chunks[2] = keccak256("chunk2");
        chunks[3] = keccak256("chunk3");
        chunks[4] = keccak256("chunk4");
        chunks[5] = keccak256("chunk5");
        chunks[6] = keccak256("chunk6");
        chunks[7] = keccak256("chunk7");

        bytes32 chunk0 = keccak256(abi.encodePacked(chunks[0], chunks[0]));
        bytes32 expectedRoot = keccak256(
            abi.encodePacked(
                keccak256(
                    abi.encodePacked(
                        keccak256(abi.encodePacked(chunk0, chunks[1])), //
                        keccak256(abi.encodePacked(chunks[2], chunks[3]))
                    )
                ),
                keccak256(
                    abi.encodePacked(
                        keccak256(abi.encodePacked(chunks[4], chunks[5])),
                        keccak256(abi.encodePacked(chunks[6], chunks[7]))
                    )
                )
            )
        );
        // [FAIL: assertion failed: 0xf386e34e7fc1bf9aa3178012232d02f781a6ddf5b2e08c330a5f457557e85627 != 0xc61f6863f580527844f8529bfe593ad9c319c641d385b6413a341ff4383032dd]
        assertEq(chunks.merkleize(), expectedRoot);

        // bytes32[] memory proof = new bytes32[](3);
        // proof[0] = keccak256("chunk2");
        // proof[1] = keccak256(abi.encodePacked(keccak256("chunk3"), keccak256("chunk4")));
        // proof[2] = keccak256(
        //     abi.encodePacked(
        //         keccak256(abi.encodePacked(keccak256("chunk5"), keccak256("chunk6"))),
        //         keccak256(abi.encodePacked(keccak256("chunk7"), keccak256("chunk8")))
        //     )
        // );

        // bool result = MerkleUtils.verifyProof(proof, root, keccak256("chunk1"), 0);
        // assertTrue(result, "The proof should be valid");
    }
}
