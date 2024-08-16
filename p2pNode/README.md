## How to test
```sh
docker build -t nodep2p .

docker volume rm p2pnode_shared_volume
docker compose up -d
```
