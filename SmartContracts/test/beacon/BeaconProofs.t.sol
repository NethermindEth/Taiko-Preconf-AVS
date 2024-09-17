// SPDX-License-Identifier: UNLICENSED
pragma solidity 0.8.25;

import {BaseTest} from "../BaseTest.sol";
import {MerkleUtils} from "src/libraries/MerkleUtils.sol";

/// @dev The beacon chain data used here is from slot 9000000 on Ethereum mainnet.
contract BeaconProofs is BaseTest {
    function test_beaconProofs_validatorInclusionInValidatorList() external pure {
        bytes32[8] memory validatorChunks;
        validatorChunks[0] = 0x8d7c2b324f41a1d395fc265d42c6e1293b38c33a674244cae9ac67d68367036d;
        validatorChunks[1] = 0x0100000000000000000000006661be71769ff00c5e403f327869505caf0b7f70;
        validatorChunks[2] = 0x0040597307000000000000000000000000000000000000000000000000000000;
        validatorChunks[3] = 0x0000000000000000000000000000000000000000000000000000000000000000;
        validatorChunks[4] = 0xe271030000000000000000000000000000000000000000000000000000000000;
        validatorChunks[5] = 0x6084030000000000000000000000000000000000000000000000000000000000;
        validatorChunks[6] = 0xffffffffffffffff000000000000000000000000000000000000000000000000;
        validatorChunks[7] = 0xffffffffffffffff000000000000000000000000000000000000000000000000;

        bytes32 validatorHashTreeRoot = MerkleUtils.merkleize(validatorChunks);

        bytes32[] memory validatorProof = new bytes32[](41);
        validatorProof[0] = 0xf5ee350215176477a7fb48aa80292de237856ad3068f46728da26aedca8a3b2b;
        validatorProof[1] = 0xfbeca4cff4f86c2ff5f1ff6808f57b12e7a6f3365d59a35c90f19715995f8be8;
        validatorProof[2] = 0x06ee0000b0cf0c0531c2a4f3368eb8df6079216bb6cca127a76e459c62058615;
        validatorProof[3] = 0x5b8c291888e7936b46e36d7b71d36c846fbfc04d48cab6beb20e23642f64ee69;
        validatorProof[4] = 0xa748ed979e88b53c303ece0946d13d2def12e003b90b562474dac1768d1d0975;
        validatorProof[5] = 0xe667bf725f0e72f47409d089248b50a9a11d08591b83374f18ed338f5c3ff964;
        validatorProof[6] = 0xd86b77a649fad1d48e109b8bc98d2a2dbc88a4b9b86c5e06878e0b980ebda3b7;
        validatorProof[7] = 0xc2db7c18d080f2b21f2c981f65414d00b0cc8542fda38233fa1c1ee33df4bbe1;
        validatorProof[8] = 0xe72e80d2ce704957f507af587e19a61ceadad2411c9728315e1f294fadae23f1;
        validatorProof[9] = 0x32f30ee3311d96e0544e2e4b0f4e1e1863d06224636ea8004e49a27280a81a11;
        validatorProof[10] = 0x89d191926d7681be7545b42b9ef95d413fbe1d8c014400c5ece8141be300b238;
        validatorProof[11] = 0x0c924ac306b692750b3285f974edf991dd4f05fff0ab3dd114430499722ff93b;
        validatorProof[12] = 0x1eb9a358bbe044159a2bed16a0b69b5b988ba0c57f2c267cfd390b3fb86fde6a;
        validatorProof[13] = 0xda60132f38fc053c26ba06136e03a861fd5e59734dc3e6cc1b69c072b9ce600a;
        validatorProof[14] = 0xcee182aa676671046ccf49213a58ef8d35e227a3adfaa146f7b71dc47c7bdd73;
        validatorProof[15] = 0xf1d0df094ceceed165886daf4c52c467710ed19a53df98ab2607629dbf7036ba;
        validatorProof[16] = 0x81917306117277e02aa4174ae73a2ec414862aced0491ec933434d9bd2279e3f;
        validatorProof[17] = 0xc562f7ffddaec138272a84b043216c1c906f68198f752ad6b80171794fcba3b5;
        validatorProof[18] = 0xcdfeaaff006b40d110ff925b18bffc36cf55543a35c84d25da0b196ea81c6029;
        validatorProof[19] = 0x8bd5e9cadc78cd0b0e0abd32a63a39596ad24e14552926bce0f6c54e39c29b99;
        validatorProof[20] = 0x6187b4f2f4b3e572fe26a6c73567ab5b1695303b0ad9dd5c9ab9679266fba2e3;
        validatorProof[21] = 0x8a8d7fe3af8caa085a7639a832001457dfb9128a8061142ad0335629ff23ff9c;
        validatorProof[22] = 0xfeb3c337d7a51a6fbf00b9e34c52e1c9195c969bd4e7a0bfd51d5c5bed9c1167;
        validatorProof[23] = 0xe71f0aa83cc32edfbefa9f4d3e0174ca85182eec9f3a09f6a6c0df6377a510d7;
        validatorProof[24] = 0x31206fa80a50bb6abe29085058f16212212a60eec8f049fecb92d8c8e0a84bc0;
        validatorProof[25] = 0x21352bfecbeddde993839f614c3dac0a3ee37543f9b412b16199dc158e23b544;
        validatorProof[26] = 0x619e312724bb6d7c3153ed9de791d764a366b389af13c58bf8a8d90481a46765;
        validatorProof[27] = 0x7cdd2986268250628d0c10e385c58c6191e6fbe05191bcc04f133f2cea72c1c4;
        validatorProof[28] = 0x848930bd7ba8cac54661072113fb278869e07bb8587f91392933374d017bcbe1;
        validatorProof[29] = 0x8869ff2c22b28cc10510d9853292803328be4fb0e80495e8bb8d271f5b889636;
        validatorProof[30] = 0xb5fe28e79f1b850f8658246ce9b6a1e7b49fc06db7143e8fe0b4f2b0c5523a5c;
        validatorProof[31] = 0x985e929f70af28d0bdd1a90a808f977f597c7c778c489e98d3bd8910d31ac0f7;
        validatorProof[32] = 0xc6f67e02e6e4e1bdefb994c6098953f34636ba2b6ca20a4721d2b26a886722ff;
        validatorProof[33] = 0x1c9a7e5ff1cf48b4ad1582d3f4e4a1004f3b20d8c5a2b71387a4254ad933ebc5;
        validatorProof[34] = 0x2f075ae229646b6f6aed19a5e372cf295081401eb893ff599b3f9acc0c0d3e7d;
        validatorProof[35] = 0x328921deb59612076801e8cd61592107b5c67c79b846595cc6320c395b46362c;
        validatorProof[36] = 0xbfb909fdb236ad2411b4e4883810a074b840464689986c3f8a8091827e17c327;
        validatorProof[37] = 0x55d8fb3687ba3ba49f342c77f5a1f89bec83d811446e1a467139213d640b6a74;
        validatorProof[38] = 0xf7210d4f8e7e1039790e7bf4efa207555a10a6db1dd4b95da313aaa88b88fe76;
        validatorProof[39] = 0xad21b516cbc645ffe34ab5de1c8aef8cd4e7f8d2b51e8e1456adc7563cda206f;
        validatorProof[40] = 0x2821150000000000000000000000000000000000000000000000000000000000;

        bytes32 validatorsRoot = 0x0ccf56d8e76d16306c6e6e78ec20c07be5fa5ae89b18873b43cc823075a5df0b;
        uint256 validatorIndex = 912203;

        vm.assertTrue(MerkleUtils.verifyProof(validatorProof, validatorsRoot, validatorHashTreeRoot, validatorIndex));
    }

    function test_beaconProofs_validatorListInclusionInBeaconState() external pure {
        bytes32[] memory beaconStateProofForValidatorList = new bytes32[](5);
        beaconStateProofForValidatorList[0] = 0x8c53160000000000000000000000000000000000000000000000000000000000;
        beaconStateProofForValidatorList[1] = 0xd9cb62ffd113d2a2b71b4539c54bf01587d8a2a5a7c81baa2c2ae89d245578d6;
        beaconStateProofForValidatorList[2] = 0xefbad4c97640101fc18122e8b818e8cc3c278a18e05dc601af4095d5519d834a;
        beaconStateProofForValidatorList[3] = 0x775d61d75ab0731115447847764383a42283b502eb4ed3ca7ba412ac67da5138;
        beaconStateProofForValidatorList[4] = 0xbb5cf5c0273b8d100f329ea0c78c471d0833f048c7fc264c285c3696d7aed412;

        bytes32 validatorListRoot = 0x0ccf56d8e76d16306c6e6e78ec20c07be5fa5ae89b18873b43cc823075a5df0b;
        bytes32 beaconStateRoot = 0xcd918afbe365c6dcabab551e32fae5f3f9677433876049dc035e5135122a2e7e;

        vm.assertTrue(MerkleUtils.verifyProof(beaconStateProofForValidatorList, beaconStateRoot, validatorListRoot, 11));
    }

    function test_beaconProofs_beaconStateInclusionInBeaconBlock() external pure {
        bytes32[] memory beaconBlockProofForBeaconState = new bytes32[](3);
        beaconBlockProofForBeaconState[0] = 0xf47de6dfa04049ce0586d989821321111d896f3cc37e40637fc226bee212e43d;
        beaconBlockProofForBeaconState[1] = 0x7506bc99ed6f0e48ad0e1ded3e878dfcfe08ca4a89308910ba1941e912673258;
        beaconBlockProofForBeaconState[2] = 0x00f48b46fd6aac7f8a72d8e1eed4f3b5bd244bf6242cb538ca94b44aed02857a;

        bytes32 beaconStateRoot = 0xcd918afbe365c6dcabab551e32fae5f3f9677433876049dc035e5135122a2e7e;
        bytes32 beaconBlockRoot = 0xcc8a36da0d5112c8dd602530ac7c7b8364edfd92cdc6f0d62365de392e8e5bb6;

        vm.assertTrue(MerkleUtils.verifyProof(beaconBlockProofForBeaconState, beaconBlockRoot, beaconStateRoot, 3));
    }

    function test_beaconProofs_proposerInclusionInBeaconBlock() external pure {
        bytes32[] memory beaconBlockProofForProposer = new bytes32[](3);
        beaconBlockProofForProposer[0] = 0x4054890000000000000000000000000000000000000000000000000000000000;
        beaconBlockProofForProposer[1] = 0xd22083672621f940e26b3f1e627f8c311a3f5f0874c193b40974f244668e1372;
        beaconBlockProofForProposer[2] = 0x00f48b46fd6aac7f8a72d8e1eed4f3b5bd244bf6242cb538ca94b44aed02857a;

        uint256 validatorIndex = 912203;
        bytes32 beaconBlockRoot = 0xcc8a36da0d5112c8dd602530ac7c7b8364edfd92cdc6f0d62365de392e8e5bb6;

        vm.assertTrue(
            MerkleUtils.verifyProof(
                beaconBlockProofForProposer, beaconBlockRoot, MerkleUtils.toLittleEndian(validatorIndex), 1
            )
        );
    }
}
