set -e

export PRIVATE_KEY=0xbcdf20249abf0ed6d944c0288fad489e33f66b3960d9e6229c1cd214ed3bbe31
export FORK_URL=http://127.0.0.1:8545

forge script scripts/deployment/DeployEigenlayerMVP.s.sol:DeployEigenlayerMVP \
  --rpc-url $FORK_URL \
  --broadcast \
  --skip-simulation \
  --private-key $PRIVATE_KEY