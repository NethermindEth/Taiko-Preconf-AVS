## How to test
```sh
docker build -t nodep2p .
docker build -t bootnodep2p ./p2pBootNode

docker volume rm p2pnode_shared_volume
docker compose up -d
```
