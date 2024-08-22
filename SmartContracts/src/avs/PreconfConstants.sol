// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

library PreconfConstants {
    uint256 internal constant SECONDS_IN_SLOT = 12;
    uint256 internal constant SECONDS_IN_EPOCH = 384; // 32 slots * 12 seconds
    uint256 internal constant TWO_EPOCHS = 768;
    uint256 internal constant BEACON_GENESIS_TIMESTAMP = 1606824023; // Dec-01-2020 12:00:23 PM +UTC
    uint256 internal constant DISPUTE_PERIOD = 2 * SECONDS_IN_EPOCH;
}
