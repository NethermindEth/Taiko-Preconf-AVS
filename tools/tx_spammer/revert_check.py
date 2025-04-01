from web3 import Web3
import argparse

def main():
    parser = argparse.ArgumentParser(description='Check transaction revert status')
    parser.add_argument('--tx-hash', type=str, required=True,
                        help='Transaction hash to check (e.g., 0x1234...)')
    parser.add_argument('--rpc-url', type=str, required=True,
                        help='HTTP or WebSocket endpoint of the Ethereum node')

    args = parser.parse_args()
    tx_hash = args.tx_hash
    rpc_url = args.rpc_url

    if rpc_url.startswith('ws://') or rpc_url.startswith('wss://'):
        w3 = Web3(Web3.LegacyWebSocketProvider(rpc_url))
    else:
        w3 = Web3(Web3.HTTPProvider(rpc_url))

    # Get the transaction and its receipt
    tx = w3.eth.get_transaction(tx_hash)
    receipt = w3.eth.get_transaction_receipt(tx_hash)

    # Check if the transaction failed
    if receipt['status'] == 0:
        # If it failed, replay the transaction to get the revert reason
        try:
            w3.eth.call(
                {
                    'to': tx['to'],
                    'from': tx['from'],
                    'data': tx['input'],
                    'value': tx['value'],
                    'gas': tx['gas'],
                    'gasPrice': tx['gasPrice'],
                },
                block_identifier=receipt['blockNumber']
            )
        except Exception as e:
            print(f"Revert reason: {str(e)}")
    else:
        print("Transaction did not fail")

if __name__ == "__main__":
    main()
