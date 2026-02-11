# Security Review: autonomous-fund

Date: 2026-02-11
Scope reviewed:
- `src/lib.rs`
- `src/types.rs`
- `src/bond_registry_proxy.rs`
- `src/uptime_proxy.rs`
- `tests/autonomous_fund_test.rs`

## Executive Summary

The contract has a clean high-level governance model, but there are several material attack surfaces around **vote accounting**, **withdrawal-time complexity**, and **parameter edge cases in views**.

Most important issues:
1. **High** — Vote manipulation because votes are snapshotted at cast-time and not adjusted while proposals are `Open`.
2. **High** — Potential gas/DoS risk in `withdraw` due to nested loops over proposals and vote records.
3. **Medium** — Quorum basis includes unowned dead shares, permanently increasing governance threshold.
4. **Medium** — View pagination underflow/overflow edge cases (`count == 0` and unchecked arithmetic).
5. **Low/Design** — Reliance on external identity/reputation contracts without freeze/version controls.

---

## Findings

## 1) Vote manipulation while proposal is Open (High)

### Where
- `vote()` stores fixed vote weight from current shares.
- `withdraw()` reduces balances immediately.
- `process_rage_quit()` only adjusts votes for proposals already in `Passed` status.

### Why this matters
A member can:
1. Deposit large temporary capital.
2. Vote with high weight while proposal is `Open`.
3. Withdraw before `finalizeVoting`.

Because vote weights are not updated for `Open` proposals after withdrawal, `yes_votes`/`no_votes` can remain inflated versus current share ownership. Finalization then compares stale vote totals against current `total_shares`, enabling governance influence with temporary liquidity.

### Attack vector
- Temporary capital holder (or coordinated set) casts decisive votes.
- Exits before voting deadline.
- Proposal still passes due to stale vote totals.

### Recommended mitigation
- Use snapshot-based governance:
  - either lock voting power for voting window,
  - or recompute effective voting weight at finalize from snapshot balances,
  - or apply vote weight decay/reduction when shares change during `Open` period.

---

## 2) Withdrawal gas DoS via `process_rage_quit` nested iteration (High)

### Where
- `withdraw()` calls `process_rage_quit()`.
- `process_rage_quit()` loops all proposals `1..=proposal_count`.
- For each `Passed` proposal in timelock, loops entire `vote_records` until matching voter.

### Why this matters
A malicious actor can create many proposals and large vote record sets, increasing withdrawal cost for everyone. Since `withdraw` always triggers this processing, users may be unable to exit in high-state conditions due to gas limits.

### Attack vector
- Flood governance with many proposals + votes.
- Honest members attempt withdraw; execution becomes too expensive / may fail.

### Recommended mitigation
- Replace global scanning with per-user indexing:
  - maintain mapping `user -> proposals_voted`.
  - only iterate relevant proposals for withdrawing user.
- Add hard limits to vote-record scans per call, with resumable processing state.

---

## 3) Quorum includes dead shares forever (Medium)

### Where
- On first deposit, `DEAD_SHARES` are added to `total_shares` and assigned to no account.
- Quorum is calculated as `51% * total_shares`.

### Why this matters
Dead shares are a standard anti-inflation pattern, but including them in quorum raises governance threshold permanently. In low-liquidity phases this can make governance harder to pass than intended and can be used as a governance-friction vector.

### Recommended mitigation
- Compute quorum on `circulating_shares = total_shares - DEAD_SHARES` (bounded at 0).
- Or mint dead shares to a known sink and explicitly exclude sink balance from quorum base.

---

## 4) Pagination arithmetic edge cases in views (Medium)

### Where
- `get_proposals(from, count)` computes `start + count - 1`.
- `get_members(from, count)` computes `start + count as usize` without overflow checks.

### Why this matters
- If `count == 0`, `start + count - 1` underflows in unsigned arithmetic.
- Large values can overflow intermediate arithmetic.

These are view endpoints (read-only), but can still break API integrations and off-chain indexers.

### Recommended mitigation
- Guard `require!(count > 0, "count must be > 0")` or return empty early.
- Use checked/saturating math for indices.

---

## 5) External dependency trust boundary not pinned (Low / Design)

### Where
- `deposit()` relies on external `BondRegistry` and `Uptime` readonly calls.

### Why this matters
If those contracts are upgraded or compromised, membership admission logic can be altered externally without changes in this contract.

### Recommended mitigation
- Governance-controlled address rotation endpoint with timelock + eventing.
- Optional hash/version checks if ecosystem supports it.
- Operational monitoring/alerts on dependency changes.

---

## Additional observations

- **No owner backdoor** is a strong governance property, but also means no emergency pause if dependencies fail.
- **Proposal spam** risk exists because proposal creation has no explicit rate-limit/deposit-bond.
- **Testing coverage** currently validates buildability only; no behavioral tests for voting/withdraw/rage-quit edge cases.

## Suggested next steps (priority order)

1. Fix vote-weight accounting model for `Open` proposals (snapshot or lock-based).
2. Redesign rage-quit processing to avoid O(total proposals × votes) scans on withdrawal.
3. Add parameter guards and checked math for view pagination.
4. Expand scenario tests to include adversarial flows:
   - vote then withdraw before finalize,
   - many proposals/votes then withdraw,
   - quorum behavior with dead shares.
5. Add dependency-governance process for external registry/uptime contracts.

