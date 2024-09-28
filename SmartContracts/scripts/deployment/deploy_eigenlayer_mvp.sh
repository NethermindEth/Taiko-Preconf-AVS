set -e

forge script scripts/deployment/DeployEigenlayerMVP.s.sol:DeployEigenlayerMVP \
  --rpc-url $FORK_URL \
  --broadcast \
  --skip-simulation \
  --private-key $PRIVATE_KEY