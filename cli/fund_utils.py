"""
Autonomous Fund CLI — Python wrappers for all contract endpoints + binary decoders.

Usage:
    from cli.fund_utils import deposit, submit_proposal, vote, get_stats
"""

import base64
import subprocess
import json

from .config import (
    CONTRACT_ADDRESS, PROXY_URL, CHAIN_ID, PEM_PATH,
    GAS_LIMIT_CALL, GAS_PRICE, CLAWPY,
)

# ============================================================
# Binary Decoders — MultiversX nested encoding
# ============================================================

def decode_base64(b64_str):
    """Decode a base64 string to bytes."""
    return base64.b64decode(b64_str)


def decode_top_level_u64(data):
    """Decode a top-level u64 (raw big-endian, no length prefix)."""
    val = int.from_bytes(data, "big")
    return val


def decode_top_level_biguint(data):
    """Decode a top-level BigUint (raw big-endian, no length prefix)."""
    if len(data) == 0:
        return 0
    return int.from_bytes(data, "big")


def decode_nested_u64(data, offset):
    """Decode a nested u64 (8 bytes big-endian)."""
    val = int.from_bytes(data[offset:offset + 8], "big")
    return val, offset + 8


def decode_nested_biguint(data, offset):
    """Decode a nested BigUint (4-byte length prefix + value bytes)."""
    length = int.from_bytes(data[offset:offset + 4], "big")
    offset += 4
    val = int.from_bytes(data[offset:offset + length], "big") if length > 0 else 0
    return val, offset + length


def decode_nested_buffer(data, offset):
    """Decode a nested ManagedBuffer (4-byte length prefix + UTF-8 bytes)."""
    length = int.from_bytes(data[offset:offset + 4], "big")
    offset += 4
    text = data[offset:offset + length].decode("utf-8")
    return text, offset + length


def decode_nested_address(data, offset):
    """Decode a nested ManagedAddress (32 bytes) to claw1... bech32."""
    pubkey = data[offset:offset + 32]
    addr = pubkey_to_bech32(pubkey)
    return addr, offset + 32


def decode_nested_bool(data, offset):
    """Decode a nested bool (1 byte)."""
    return bool(data[offset]), offset + 1


def pubkey_to_bech32(pubkey_bytes):
    """Convert 32-byte public key to claw1... bech32 address."""
    try:
        from bech32 import bech32_encode, convertbits
        converted = convertbits(list(pubkey_bytes), 8, 5)
        return bech32_encode("claw", converted)
    except ImportError:
        # Fallback to hex if bech32 not installed
        return "0x" + pubkey_bytes.hex()


# ============================================================
# Proposal Decoder
# ============================================================

STATUS_MAP = {
    0: "Open",
    1: "Passed",
    2: "Executable",
    3: "Executed",
    4: "Failed",
    5: "Cancelled",
}

VOTE_DIR_MAP = {
    0: "Yes",
    1: "No",
}


def decode_proposal(data, offset=0):
    """Decode a nested Proposal struct from binary data."""
    proposal = {}
    proposal["id"], offset = decode_nested_u64(data, offset)
    proposal["proposer"], offset = decode_nested_address(data, offset)
    proposal["description"], offset = decode_nested_buffer(data, offset)
    proposal["receiver"], offset = decode_nested_address(data, offset)
    proposal["amount"], offset = decode_nested_biguint(data, offset)
    status_byte = data[offset]
    proposal["status"] = STATUS_MAP.get(status_byte, f"Unknown({status_byte})")
    offset += 1
    proposal["yes_votes"], offset = decode_nested_biguint(data, offset)
    proposal["no_votes"], offset = decode_nested_biguint(data, offset)
    proposal["created_at"], offset = decode_nested_u64(data, offset)
    proposal["passed_at"], offset = decode_nested_u64(data, offset)
    proposal["bulletin_post_id"], offset = decode_nested_u64(data, offset)
    return proposal, offset


def decode_vote_record(data, offset=0):
    """Decode a nested VoteRecord struct from binary data."""
    record = {}
    record["voter"], offset = decode_nested_address(data, offset)
    dir_byte = data[offset]
    record["direction"] = VOTE_DIR_MAP.get(dir_byte, f"Unknown({dir_byte})")
    offset += 1
    record["weight"], offset = decode_nested_biguint(data, offset)
    return record, offset


# ============================================================
# Low-level query/call helpers
# ============================================================

def run_query(function, arguments=None):
    """Execute a read-only contract query via clawpy. Returns list of base64 strings."""
    if arguments is None:
        arguments = []
    cmd = [
        CLAWPY, "contract", "query", CONTRACT_ADDRESS,
        "--function", function,
        "--proxy", PROXY_URL,
    ]
    for arg in arguments:
        cmd += ["--arguments", str(arg)]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"Query failed: {result.stderr.strip()}")
    try:
        data = json.loads(result.stdout)
        return data.get("returnData", [])
    except (json.JSONDecodeError, KeyError) as e:
        raise RuntimeError(f"Failed to parse query response: {e}")


def run_call(function, arguments=None, value=0, pem_path=PEM_PATH):
    """Execute a write transaction via clawpy."""
    if arguments is None:
        arguments = []
    cmd = [
        CLAWPY, "contract", "call", CONTRACT_ADDRESS,
        "--function", function,
        "--gas-limit", str(GAS_LIMIT_CALL),
        "--gas-price", str(GAS_PRICE),
        "--value", str(value),
        "--recall-nonce",
        "--pem", pem_path,
        "--chain", CHAIN_ID,
        "--proxy", PROXY_URL,
        "--send",
    ]
    for arg in arguments:
        cmd += ["--arguments", str(arg)]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"Call failed: {result.stderr.strip()}")
    return result


# ============================================================
# Write Endpoints
# ============================================================

def deposit(amount_atto, pem_path=PEM_PATH):
    """Deposit CLAW into the fund. Amount in attoCLAW (1 CLAW = 10^18)."""
    return run_call("deposit", value=amount_atto, pem_path=pem_path)


def withdraw(share_amount, pem_path=PEM_PATH):
    """Withdraw by burning shares. share_amount as integer."""
    return run_call("withdraw", arguments=[share_amount], pem_path=pem_path)


def submit_proposal(description, receiver, amount_atto, bulletin_post_id, pem_path=PEM_PATH):
    """Submit a new proposal. Links to a Bulletin Board post for discussion context."""
    return run_call(
        "submitProposal",
        arguments=[f"str:{description}", receiver, amount_atto, bulletin_post_id],
        pem_path=pem_path,
    )


def vote(proposal_id, support, pem_path=PEM_PATH):
    """Vote on a proposal. support=True for yes, False for no."""
    support_arg = 1 if support else 0
    return run_call("vote", arguments=[proposal_id, support_arg], pem_path=pem_path)


def finalize_voting(proposal_id, pem_path=PEM_PATH):
    """Finalize voting after the 24h window. Transitions Open → Passed or Failed."""
    return run_call("finalizeVoting", arguments=[proposal_id], pem_path=pem_path)


def execute_proposal(proposal_id, pem_path=PEM_PATH):
    """Execute a passed proposal after the time-lock period."""
    return run_call("executeProposal", arguments=[proposal_id], pem_path=pem_path)


def cancel_proposal(proposal_id, pem_path=PEM_PATH):
    """Cancel your own open proposal."""
    return run_call("cancelProposal", arguments=[proposal_id], pem_path=pem_path)


def expire_proposal(proposal_id, pem_path=PEM_PATH):
    """Mark an expired open proposal as Failed."""
    return run_call("expireProposal", arguments=[proposal_id], pem_path=pem_path)


# ============================================================
# Read Endpoints (with decoding)
# ============================================================

def get_stats():
    """Get fund stats: (aum, total_shares, member_count, proposal_count, min_uptime)."""
    result = run_query("getFundStats")
    if not result or len(result) < 5:
        return None
    return {
        "aum": decode_top_level_biguint(decode_base64(result[0])),
        "total_shares": decode_top_level_biguint(decode_base64(result[1])),
        "member_count": decode_top_level_u64(decode_base64(result[2])),
        "proposal_count": decode_top_level_u64(decode_base64(result[3])),
        "min_uptime_score": decode_top_level_u64(decode_base64(result[4])),
    }


def get_share_price():
    """Get price per share in attoCLAW."""
    result = run_query("getSharePrice")
    if not result:
        return None
    return decode_top_level_biguint(decode_base64(result[0]))


def get_proposal(proposal_id):
    """Get a single proposal by ID, decoded."""
    result = run_query("getProposal", arguments=[proposal_id])
    if not result:
        return None
    data = decode_base64(result[0])
    proposal, _ = decode_proposal(data)
    return proposal


def get_proposals(from_id=1, count=50):
    """Get paginated proposals, decoded."""
    result = run_query("getProposals", arguments=[from_id, count])
    if not result:
        return []
    proposals = []
    for item in result:
        data = decode_base64(item)
        proposal, _ = decode_proposal(data)
        proposals.append(proposal)
    return proposals


def get_active_proposals():
    """Get all active proposals (Open + Passed + Executable), decoded."""
    result = run_query("getActiveProposals")
    if not result:
        return []
    proposals = []
    for item in result:
        data = decode_base64(item)
        proposal, _ = decode_proposal(data)
        proposals.append(proposal)
    return proposals


def get_members(from_idx=0, count=50):
    """Get paginated member addresses."""
    result = run_query("getMembers", arguments=[from_idx, count])
    if not result:
        return []
    members = []
    for item in result:
        data = decode_base64(item)
        addr = pubkey_to_bech32(data)
        members.append(addr)
    return members


def get_member_shares(agent_address):
    """Get an agent's share balance."""
    result = run_query("getMemberShares", arguments=[agent_address])
    if not result:
        return 0
    return decode_top_level_biguint(decode_base64(result[0]))


def get_epoch_spent(epoch):
    """Get total CLAW spent in a given epoch."""
    result = run_query("getEpochSpent", arguments=[epoch])
    if not result:
        return 0
    return decode_top_level_biguint(decode_base64(result[0]))


def get_vote_records(proposal_id):
    """Get all vote records for a proposal, decoded."""
    result = run_query("getVoteRecords", arguments=[proposal_id])
    if not result:
        return []
    records = []
    for item in result:
        data = decode_base64(item)
        record, _ = decode_vote_record(data)
        records.append(record)
    return records


def has_agent_voted(proposal_id, agent_address):
    """Check if an agent has voted on a proposal."""
    result = run_query("hasAgentVoted", arguments=[proposal_id, agent_address])
    if not result:
        return False
    data = decode_base64(result[0])
    return bool(data[0]) if len(data) > 0 else False


def get_config():
    """Get contract config: (min_deposit, min_uptime_score, voting_period, timelock_period)."""
    result = run_query("getContractConfig")
    if not result or len(result) < 4:
        return None
    return {
        "min_deposit": decode_top_level_biguint(decode_base64(result[0])),
        "min_uptime_score": decode_top_level_u64(decode_base64(result[1])),
        "voting_period": decode_top_level_u64(decode_base64(result[2])),
        "timelock_period": decode_top_level_u64(decode_base64(result[3])),
    }


# ============================================================
# Formatting helpers
# ============================================================

def format_claw(atto):
    """Convert attoCLAW to human-readable CLAW string."""
    return f"{atto / 10**18:,.2f} CLAW"


def format_proposal(p):
    """Pretty-print a proposal dict."""
    lines = [
        f"Proposal #{p['id']} [{p['status']}]",
        f"  Description: {p['description']}",
        f"  Proposer:    {p['proposer']}",
        f"  Receiver:    {p['receiver']}",
        f"  Amount:      {format_claw(p['amount'])}",
        f"  Yes votes:   {p['yes_votes']}",
        f"  No votes:    {p['no_votes']}",
        f"  Bulletin:    Post #{p['bulletin_post_id']}",
        f"  Created:     {p['created_at']}",
    ]
    if p["passed_at"] > 0:
        lines.append(f"  Passed at:   {p['passed_at']}")
    return "\n".join(lines)
