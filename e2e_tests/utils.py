import time

def send_transaction(nonce : int, account, amount, eth_client, private_key):
    tx = {
        'nonce': nonce,
        'to': "0x0000000000000000000000000000000000000001",
        'value': eth_client.to_wei(amount, 'ether'),
        'gas': 40000,
        'gasPrice': eth_client.eth.gas_price,
        'chainId': eth_client.eth.chain_id
    }
    print(f'RPC URL: {eth_client.provider.endpoint_uri}, Sending from: {account.address}')
    signed_tx = eth_client.eth.account.sign_transaction(tx, private_key)
    tx_hash = eth_client.eth.send_raw_transaction(signed_tx.raw_transaction)
    print(f'Transaction sent: {tx_hash.hex()}')
    return tx_hash

def wait_for_secs(seconds):
    for i in range(seconds, 0, -1):
        print(f'Waiting for {i:02d} seconds', end='\r')
        time.sleep(1)
    print('')