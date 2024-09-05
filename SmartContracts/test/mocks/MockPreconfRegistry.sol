// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

contract MockPreconfRegistry {
    mapping(address preconfer => uint256 index) internal preconferToIndex;
    mapping(uint256 index => address preconfer) internal indexToPreconfer;

    uint256 internal nextPreconferIndex = 1;

    function registerPreconfer(address preconfer) external {
        uint256 _nextPreconferIndex = nextPreconferIndex;

        preconferToIndex[preconfer] = _nextPreconferIndex;
        indexToPreconfer[_nextPreconferIndex] = preconfer;

        unchecked {
            nextPreconferIndex = _nextPreconferIndex + 1;
        }
    }

    function getNextPreconferIndex() external view returns (uint256) {
        return nextPreconferIndex;
    }

    function getPreconferIndex(address preconfer) external view returns (uint256) {
        return preconferToIndex[preconfer];
    }

    function getPreconferAtIndex(uint256 index) external view returns (address) {
        return indexToPreconfer[index];
    }
}
