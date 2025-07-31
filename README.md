![GitHub License](https://img.shields.io/github/license/NethermindEth/Catalyst)
![Docker Stars](https://img.shields.io/docker/stars/nethswitchboard/catalyst-node)
![Docker Pulls](https://img.shields.io/docker/pulls/nethswitchboard/catalyst-node)
![Docker Image Size (tag)](https://img.shields.io/docker/image-size/nethswitchboard/catalyst-node/latest)



# Catalyst: A Taiko Stack preconfer sidecar

Engineered with ❤️ at [Nethermind](https://www.nethermind.io/)

## Features

- ✅ **Validator registration** to the preconfirmation registry at initial setup
- ✅ **Lookahead** submissions and disputes.
- ✅ **Dispute** **against preconfirmations** made by other validators.
- ✅ Execution of the **main preconfirmation duties**, which include:
  - ✅ Checking the lookahead to determine if it's the validator’s turn to preconfirm.
  - ✅ Constructing L2 blocks using the Taiko mempool.
  - ✅ Publishing the L2 block to a preconfirmation P2P network.
  - ✅ Syncing the local Taiko head with the latest preconfirmation state.
  - ✅ Posting L2 blocks through the L1 mempool

## Docker image

### Use the pre-built image

```sh
docker pull nethswitchboard/catalyst-node:latest
```

[The image](https://hub.docker.com/r/nethswitchboard/catalyst-node) is built with [this Github Action](https://github.com/NethermindEth/Catalyst/blob/master/.github/workflows/catalyst_docker_build.yml).

### Build the image locally

```sh
docker build -t node .
```

## Git hooks

Hooks require additional dependencies:
```sh
 cargo install typos-cli cargo-sort cargo-deny
```

## License

MIT. The license is also applied to all commits made before the license introduced.

## Would like to contribute?

see [Contributing](./CONTRIBUTING.md).
