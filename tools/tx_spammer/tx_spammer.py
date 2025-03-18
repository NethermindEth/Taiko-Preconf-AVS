r"""
This script is used to spam transactions on the Taiko network. It reads the private key and recipient address from a .env file,
connects to the Taiko network, and sends a specified number of transactions to the recipient address.

Setup:
1. Create a virtual environment:
   python -m venv venv

2. Activate the virtual environment:
   - On Windows: venv\Scripts\activate
   - On macOS/Linux: source venv/bin/activate

3. Install the required dependencies:
   pip install -r requirements.txt

4. Create a .env file in the tools/tx_spammer directory with the following content:
   PRIVATE_KEY=<your_private_key>
   RECIPIENT_ADDRESS=<recipient_address>

5. Run the script:
   python tx_spammer.py [--count COUNT] [--amount AMOUNT] [--rpc RPC_URL]

CLI Parameters:
--count: Number of transactions to send (default: 1)
--amount: Amount of ETH to send per transaction (default: 0.006)
--rpc: RPC URL for the Taiko network (default: https://RPC.helder.taiko.xyz)
"""



import time
from web3 import Web3
import os
from dotenv import load_dotenv
import argparse
import requests
import json

# Load environment variables from .env file
load_dotenv()

def get_beacon_genesis_timestamp(beacon_rpc_url):
    try:
        response = requests.get(f"{beacon_rpc_url}/eth/v1/beacon/genesis")
        response.raise_for_status()
        genesis_data = json.loads(response.text)
        return int(genesis_data['data']['genesis_time'])
    except requests.RequestException as e:
        print(f"Error fetching beacon genesis timestamp: {e}")
        return None

private_key = os.getenv('PRIVATE_KEY')
if not private_key:
    raise Exception("Environment variable PRIVATE_KEY not set")

recipient = os.getenv('RECIPIENT_ADDRESS')
if not recipient:
    raise Exception("Environment variable RECIPIENT_ADDRESS not set")

parser = argparse.ArgumentParser(description='Spam transactions on the Taiko network.')
parser.add_argument('--count', type=int, default=1, help='Number of transactions to send')
parser.add_argument('--amount', type=float, default=0.006, help='Amount of ETH to send per transaction')
parser.add_argument('--rpc', type=str, default='https://RPC.helder.taiko.xyz', help='RPC URL for the Taiko network')
parser.add_argument('--slots', nargs='+', type=int, default=[],
                    help='Slots to send transactions (0-31)')
parser.add_argument('--beacon-rpc', type=str, help='Beacon RPC URL for the Taiko network')
parser.add_argument('--sleep', type=float, default=2.0, help='Sleep time between transactions in seconds')
args = parser.parse_args()


genesis_timestamp = None
if len(args.slots) > 0:
    if args.beacon_rpc is None:
        raise Exception("Beacon RPC URL is required when specifying slots")
    print(f'Sending transactions for slots: {args.slots}')
    genesis_timestamp = get_beacon_genesis_timestamp(args.beacon_rpc)
    if genesis_timestamp is None:
        raise Exception("Failed to get beacon genesis timestamp")
    else:
        print(f'Beacon genesis timestamp: {genesis_timestamp}')

# Connect to the Taiko network
w3 = Web3(Web3.HTTPProvider(args.rpc))

# Check if connected
if not w3.is_connected():
    raise Exception("Failed to connect to the Taiko network")

# Get the account from the private key
account = w3.eth.account.from_key(private_key)
amount = w3.to_wei(args.amount, 'ether')

def send_transaction(nonce : int):
    tx = {
        'nonce': nonce,
        'to': recipient,
        'value': amount,
        'gas': 40000,
        'gasPrice': w3.to_wei('10', 'gwei'),
        'chainId': w3.eth.chain_id
    }
    print(f'Sending transaction: {tx} by RPC: {args.rpc}')
    print(f'Sending from: {account.address}')
    signed_tx = w3.eth.account.sign_transaction(tx, private_key)
    tx_hash = w3.eth.send_raw_transaction(signed_tx.raw_transaction)
    print(f'Transaction sent: {tx_hash.hex()}')

def spam_transactions(count):
    nonce = w3.eth.get_transaction_count(account.address)
    for _ in range(count):
        send_transaction(nonce)
        nonce += 1
        time.sleep(args.sleep)


if len(args.slots) > 0:
    for slot in args.slots:
        current_time = int(time.time())
        current_slot = (current_time - genesis_timestamp) // 12  # Assuming 12-second slot time
        current_epoch_slot = current_slot % 32
        print(f'Current slot: {current_slot}, current_epoch_slot: {current_epoch_slot}')

        # Calculate the time until the next occurrence of the desired slot
        time_since_epoch_start = (current_time - genesis_timestamp) % (32 * 12)
        time_until_slot = ((slot - current_epoch_slot) % 32) * 12 - (time_since_epoch_start % 12)

        if time_until_slot <= 0:
            time_until_slot += 32 * 12  # Wait for the next epoch if we've missed the slot in this epoch

        print(f'Waiting {time_until_slot} seconds for slot {slot}')
        time.sleep(time_until_slot)

        print(f'Spamming transactions for slot {slot}')
        spam_transactions(args.count)
else:
    spam_transactions(args.count)