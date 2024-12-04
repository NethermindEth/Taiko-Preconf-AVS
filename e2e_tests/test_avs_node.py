import pytest
import requests
from web3 import Web3
import os
from dotenv import load_dotenv
import sys
from utils import *

load_dotenv()

l2_prefunded_priv_key = os.getenv("TEST_L2_PREFUNDED_PRIVATE_KEY")
if not l2_prefunded_priv_key:
    raise Exception("Environment variable TEST_L2_PREFUNDED_PRIVATE_KEY not set")


def test_chain_ids(l1_client, l2_client_node1, l2_client_node2):
    """Test to verify the chain IDs of L1 and L2 networks"""
    l1_chain_id = l1_client.eth.chain_id
    l2_chain_id_node1 = l2_client_node1.eth.chain_id
    l2_chain_id_node2 = l2_client_node2.eth.chain_id

    print(f"L1 Chain ID: {l1_chain_id}")
    print(f"L2 Chain ID Node 1: {l2_chain_id_node1}")
    print(f"L2 Chain ID Node 2: {l2_chain_id_node2}")

    assert l1_chain_id > 0, "L1 chain ID should be greater than 0"
    assert l2_chain_id_node1 > 0, "L2 chain ID should be greater than 0"

    assert l1_chain_id != l2_chain_id_node1, "L1 and L2 should have different chain IDs"
    assert l2_chain_id_node1 == l2_chain_id_node2, "L2 nodes should have the same chain IDs"

def test_preconfirm_transaction(l1_client, l2_client_node1):
    account = l2_client_node1.eth.account.from_key(l2_prefunded_priv_key)
    nonce = l2_client_node1.eth.get_transaction_count(account.address)
    l1_block_number = l1_client.eth.block_number
    l2_block_number = l2_client_node1.eth.block_number

    send_transaction(nonce, account, '0.00005', l2_client_node1, l2_prefunded_priv_key)

    wait_for_secs(12)

    l1_block_number_after = l1_client.eth.block_number
    l2_block_number_after = l2_client_node1.eth.block_number

    print(f"L1 Block Number: {l1_block_number}")
    print(f"L2 Block Number: {l2_block_number}")
    print(f"L1 Block Number After: {l1_block_number_after}")
    print(f"L2 Block Number After: {l2_block_number_after}")

    assert l1_block_number_after > l1_block_number, "L1 block number should increase after sending a transaction"
    assert l2_block_number_after > l2_block_number, "L2 block number should increase after sending a transaction"

def test_p2p_preconfirmation(l2_client_node1, l2_client_node2):
    account = l2_client_node1.eth.account.from_key(l2_prefunded_priv_key)
    nonce = l2_client_node1.eth.get_transaction_count(account.address)
    l2_node_1_block_number = l2_client_node1.eth.block_number
    l2_node_2_block_number = l2_client_node2.eth.block_number

    send_transaction(nonce, account, '0.00006', l2_client_node1, l2_prefunded_priv_key)

    wait_for_secs(4)

    l2_node_1_block_number_after = l2_client_node1.eth.block_number
    l2_node_2_block_number_after = l2_client_node2.eth.block_number

    print(f"L2 Node 1 Block Number: {l2_node_1_block_number}")
    print(f"L2 Node 2 Block Number: {l2_node_2_block_number}")
    print(f"L2 Node 1 Block Number After: {l2_node_1_block_number_after}")
    print(f"L2 Node 2 Block Number After: {l2_node_2_block_number_after}")

    assert l2_node_2_block_number_after > l2_node_2_block_number, "L2 Node 2 block number should increase after sending a transaction"