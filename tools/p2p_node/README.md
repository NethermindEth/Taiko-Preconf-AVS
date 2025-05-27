# p2p network emulator

## How to test

```sh
cd ../../
docker build -f tools/p2p_node/Dockerfile -t nodep2p .
docker build -f tools/p2p_node/p2p_boot_node/Dockerfile -t bootnodep2p .

docker compose -f tools/p2p_node/docker-compose.yml up -d
```