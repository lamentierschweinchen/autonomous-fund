# Autonomous Fund / DAO Treasury

An on-chain venture fund for AI agents on the **Claws Network**.

## Features
- **Capital Pooling**: Agents deposit $CLAW and receive shares (AF tokens).
- **Weighted Voting**: Governance decisions are token-weighted based on share ownership.
- **Investment Proposals**: Any member can propose to allocate capital to external addresses (e.g., DEX pools).
- **Management Fee**: 2% AUM fee for the fund manager.
- **Real-time Dashboard**: Terminal-style UI for tracking fund performance.

## Structure
- `src/`: Smart contract logic (Rust).
- `frontend/`: Single-page dashboard (HTML/JS).
- `cli/`: Python scripts for agent automation.

## Quick Start

### 1. Build & Deploy
```bash
# Build
sc-meta all build

# Deploy
clawpy contract deploy --bytecode=output/autonomous-fund.wasm --pem=wallet.pem --send
```

### 2. Frontend
Open `frontend/index.html` in any browser. Enter the deployed contract address and click "Connect".

### 3. CLI (for Agents)
```python
from cli.fund_utils import deposit, submit_proposal

# Join the fund
deposit(1000000000000000000) # 1 CLAW

# Submit an investment proposal
submit_proposal("Invest in OpenDex", "claw1...", 500000000000000000)
```

## Security
- Weighted voting ensures large stakeholders have proportional influence.
- Proportional withdrawals ensure fair distribution of profits.
- Checked arithmetic prevents overflows in financial logic.
