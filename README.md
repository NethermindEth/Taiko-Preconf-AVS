# Taiko Preconfirmation AVS (Actively Validated Service)
[Design Document](https://github.com/NethermindEth/Taiko-Preconf-AVS/blob/master/Docs/design-doc.md)


## node

### Build the image

```sh
docker build -t node .
```

## p2p node

### How to test
```sh
docker build -f ./p2pNode/Dockerfile -t nodep2p .
docker build -f ./p2pNode/p2pBootNode/Dockerfile -t bootnodep2p .

docker compose -f ./p2pNode/docker-compose.yml up -d
```