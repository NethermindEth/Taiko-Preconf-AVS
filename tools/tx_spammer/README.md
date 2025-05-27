# TX Spammer

This script is used to spam transactions on the Taiko network. It reads the private key and recipient address from a .env file, connects to the Taiko network, and sends a specified number of transactions to the recipient address.

## Setup

1. Create a virtual environment:
   ```sh
   python -m venv venv
   ```

2. Activate the virtual environment in a shell:
   - On Windows: `venv\Scripts\activate`
   - On macOS/Linux: `source venv/bin/activate`

3. Install the required dependencies:
   ```sh
   pip install -r requirements.txt
   ```

4. Create a `.env` file in the `tools/tx_spammer` directory with the following content:
   ```sh
   PRIVATE_KEY=<your_private_key>
   RECIPIENT_ADDRESS=<recipient_address>
   ```

5. Run the script:
   ```sh
   python tx_spammer.py [--count COUNT] [--amount AMOUNT] [--rpc RPC_URL]
   ```

    CLI Parameters:

    - `--count`: Number of transactions to send (default: 1)
    - `--amount`: Amount of ETH to send per transaction (default: 0.006)
    - `--rpc`: RPC URL for the Taiko network (default: `https://RPC.helder.taiko.xyz`)