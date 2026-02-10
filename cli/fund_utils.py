import base64
import struct
import subprocess
import json
from .config import *

def run_query(function, arguments=[]):
    """Execute read-only contract query."""
    cmd = [CLAWPY, "contract", "query", CONTRACT_ADDRESS,
           "--function", function,
           "--proxy", PROXY_URL]
    for arg in arguments:
        cmd += ["--arguments", str(arg)]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"Error: {result.stderr}")
        return []
    try:
        data = json.loads(result.stdout)
        return data.get("data", {}).get("data", {}).get("returnData", [])
    except:
        return []

def run_call(function, arguments=[], value=0, pem_path=PEM_PATH):
    """Execute write transaction."""
    cmd = [CLAWPY, "contract", "call", CONTRACT_ADDRESS,
           "--function", function,
           "--gas-limit", str(GAS_LIMIT_CALL),
           "--gas-price", str(GAS_PRICE),
           "--value", str(value),
           "--recall-nonce",
           "--pem", pem_path,
           "--chain", CHAIN_ID,
           "--proxy", PROXY_URL,
           "--send"]
    for arg in arguments:
        cmd += ["--arguments", str(arg)]
    return subprocess.run(cmd, capture_output=True, text=True)

def deposit(amount_atto, pem_path=PEM_PATH):
    return run_call("deposit", value=amount_atto, pem_path=pem_path)

def withdraw(share_amount, pem_path=PEM_PATH):
    return run_call("withdraw", arguments=[share_amount], pem_path=pem_path)

def submit_proposal(description, receiver, amount_atto, pem_path=PEM_PATH):
    return run_call("submitProposal", 
                    arguments=[f"str:{description}", receiver, amount_atto], 
                    pem_path=pem_path)

def vote(proposal_id, pem_path=PEM_PATH):
    return run_call("vote", arguments=[proposal_id], pem_path=pem_path)

def get_stats():
    return run_query("getFundStats")

def get_proposals():
    return run_query("getProposals")
