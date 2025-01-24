set -e

: "${PRIVATE_KEY:?Environment variable PRIVATE_KEY is required}"
: "${FORK_URL:?Environment variable FORK_URL is required}"

# Check if EVM_VERSION is set and not empty
if [ -n "$EVM_VERSION" ]; then
    EVM_VERSION_FLAG="--evm-version $EVM_VERSION"
else
    EVM_VERSION_FLAG=""
fi

forge script scripts/deployment/DeployEigenlayerMVP.s.sol:DeployEigenlayerMVP \
  --rpc-url $FORK_URL \
  --broadcast \
  --skip-simulation \
  --private-key $PRIVATE_KEY \
  $EVM_VERSION_FLAG