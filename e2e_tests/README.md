# End to end preconfirmation tests

This is a collection of end to end tests for the preconfirmation service.

It requires full stack to be up and running. Usually by running
```
kurtosis run --enclave taiko-preconf-devnet . --args-file network_params.yaml
```
from the main branch of https://github.com/NethermindEth/preconfirm-devnet-package.

It also requires a `.env` file to be present in the root directory. You can copy `.env.example` file into `.env` and fill in the required values.

To run all tests:
```
pytest
```

To run a specific test with output printed:
```
pytest -s -v -k test_name
```