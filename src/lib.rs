#![no_std]

multiversx_sc::imports!();

pub mod types;
pub mod bond_registry_proxy;
pub mod uptime_proxy;

use types::{Proposal, ProposalStatus, VoteDirection, VoteRecord};

// ============================================================
// Constants
// ============================================================

/// 51% quorum — yes votes must be >= 51% of total shares AND yes > no
const QUORUM_PERCENTAGE: u64 = 51;

/// Maximum single proposal can request: 15% of AUM (1500 basis points)
const MAX_PROPOSAL_BPS: u64 = 1_500;

/// Maximum total spend per epoch: 25% of AUM (2500 basis points)
const MAX_EPOCH_SPEND_BPS: u64 = 2_500;

/// Basis points denominator
const BPS_DENOMINATOR: u64 = 10_000;

/// Voting window: 24 hours in seconds
const VOTING_PERIOD: u64 = 86_400;

/// Time-lock after passing: 24 hours in seconds
const TIMELOCK_PERIOD: u64 = 86_400;

/// Dead shares minted on first deposit to prevent inflation attack
const DEAD_SHARES: u64 = 1_000;

// ============================================================
// Contract
// ============================================================

#[multiversx_sc::contract]
pub trait AutonomousFund {
    // ========================================================
    // Init / Upgrade
    // ========================================================

    #[init]
    fn init(
        &self,
        bond_registry_address: ManagedAddress,
        uptime_address: ManagedAddress,
        min_deposit: BigUint,
        min_uptime_score: u64,
    ) {
        self.bond_registry_address().set(&bond_registry_address);
        self.uptime_address().set(&uptime_address);
        self.min_deposit().set(&min_deposit);
        self.min_uptime_score().set(min_uptime_score);
        self.total_shares().set(BigUint::zero());
        self.proposal_count().set(0u64);
    }

    #[upgrade]
    fn upgrade(&self) {}

    // ========================================================
    // ENDPOINT: deposit
    // Three-gate membership: identity + reputation + capital
    // ========================================================

    #[endpoint(deposit)]
    #[payable("EGLD")]
    fn deposit(&self) {
        let caller = self.blockchain().get_caller();
        let payment_amount = self.call_value().egld_value().clone_value();

        // ── Gate 3: Capital ──
        require!(
            payment_amount >= self.min_deposit().get(),
            "Below minimum deposit"
        );

        // ── Gate 1: Identity — must be registered in Bond Registry ──
        let bond_registry_addr = self.bond_registry_address().get();
        let agent_name: ManagedBuffer = self
            .tx()
            .to(&bond_registry_addr)
            .typed(bond_registry_proxy::BondRegistryProxy)
            .get_agent_name(caller.clone())
            .returns(ReturnsResult)
            .sync_call_readonly();
        require!(!agent_name.is_empty(), "Not a registered agent");

        // ── Gate 2: Reputation — must have minimum uptime score ──
        let uptime_addr = self.uptime_address().get();
        let lifetime_info: MultiValue4<u64, u64, u64, u64> = self
            .tx()
            .to(&uptime_addr)
            .typed(uptime_proxy::UptimeProxy)
            .get_lifetime_info(caller.clone())
            .returns(ReturnsResult)
            .sync_call_readonly();
        let (_total_heartbeats, lifetime_score, _time_since_last, _time_remaining) =
            lifetime_info.into_tuple();
        require!(
            lifetime_score >= self.min_uptime_score().get(),
            "Insufficient uptime reputation"
        );

        // ── Share calculation ──
        let current_aum = self
            .blockchain()
            .get_sc_balance(&EgldOrEsdtTokenIdentifier::egld(), 0);
        let total_shares = self.total_shares().get();

        let shares_to_mint = if total_shares == 0u64 {
            // First depositor: mint dead shares to zero address to prevent inflation attack
            let dead = BigUint::from(DEAD_SHARES);
            self.total_shares().set(&dead);
            // Shares = payment amount (1:1 for first deposit)
            // Total shares after this = DEAD_SHARES + payment_amount
            payment_amount.clone()
        } else {
            // shares = payment * total_shares / aum_before_deposit
            let aum_before = &current_aum - &payment_amount;
            require!(aum_before > 0u64, "Fund is insolvent");
            (&payment_amount * &total_shares) / &aum_before
        };

        require!(shares_to_mint > 0u64, "Deposit too small for shares");

        self.shares(&caller).update(|s| *s += &shares_to_mint);
        self.total_shares().update(|ts| *ts += &shares_to_mint);
        self.members().insert(caller.clone());

        self.deposit_event(&caller, &payment_amount, &shares_to_mint);
    }

    // ========================================================
    // ENDPOINT: withdraw
    // Agents can exit at any time. During time-lock, this
    // triggers rage-quit logic on passed proposals.
    // ========================================================

    #[endpoint(withdraw)]
    fn withdraw(&self, share_amount: BigUint) {
        let caller = self.blockchain().get_caller();
        let user_shares = self.shares(&caller).get();
        require!(
            share_amount > 0u64 && share_amount <= user_shares,
            "Invalid share amount"
        );

        let total_shares = self.total_shares().get();
        let current_aum = self
            .blockchain()
            .get_sc_balance(&EgldOrEsdtTokenIdentifier::egld(), 0);

        // payout = share_amount * current_aum / total_shares
        let payout = (&share_amount * &current_aum) / &total_shares;
        require!(payout > 0u64, "Nothing to withdraw");

        // Update shares
        self.shares(&caller).update(|s| *s -= &share_amount);
        self.total_shares().update(|ts| *ts -= &share_amount);

        // Remove from members if fully withdrawn
        let remaining = self.shares(&caller).get();
        if remaining == 0u64 {
            self.members().swap_remove(&caller);
        }

        // ── Rage-quit: retroactively remove votes from Passed proposals in time-lock ──
        self.process_rage_quit(&caller);

        self.send().direct_egld(&caller, &payout);
        self.withdraw_event(&caller, &payout, &share_amount);
    }

    // ========================================================
    // ENDPOINT: submitProposal
    // Any member can propose. Links to Bulletin Board discussion.
    // ========================================================

    #[endpoint(submitProposal)]
    fn submit_proposal(
        &self,
        description: ManagedBuffer,
        receiver: ManagedAddress,
        amount: BigUint,
        bulletin_post_id: u64,
    ) -> u64 {
        let caller = self.blockchain().get_caller();
        require!(
            self.members().contains(&caller),
            "Only members can propose"
        );

        // ── Guardrail: per-proposal cap at 15% of AUM ──
        let current_aum = self
            .blockchain()
            .get_sc_balance(&EgldOrEsdtTokenIdentifier::egld(), 0);
        let max_amount = (&current_aum * MAX_PROPOSAL_BPS) / BPS_DENOMINATOR;
        require!(
            amount <= max_amount,
            "Exceeds 15% of AUM per-proposal cap"
        );

        let proposal_id = self.proposal_count().get() + 1u64;
        let timestamp = self.blockchain().get_block_timestamp();

        let proposal = Proposal {
            id: proposal_id,
            proposer: caller.clone(),
            description,
            receiver,
            amount,
            status: ProposalStatus::Open,
            yes_votes: BigUint::zero(),
            no_votes: BigUint::zero(),
            created_at: timestamp,
            passed_at: 0u64,
            bulletin_post_id,
        };

        self.proposals(proposal_id).set(&proposal);
        self.proposal_count().set(proposal_id);

        self.proposal_created_event(proposal_id, &caller, bulletin_post_id, timestamp);

        proposal_id
    }

    // ========================================================
    // ENDPOINT: vote
    // Yes/No voting weighted by share balance.
    // ========================================================

    #[endpoint(vote)]
    fn vote(&self, proposal_id: u64, support: bool) {
        let caller = self.blockchain().get_caller();
        require!(
            self.members().contains(&caller),
            "Only members can vote"
        );
        require!(
            !self.proposals(proposal_id).is_empty(),
            "Proposal does not exist"
        );
        require!(
            !self.has_voted(proposal_id, &caller).get(),
            "Already voted"
        );

        let mut proposal = self.proposals(proposal_id).get();
        require!(
            proposal.status == ProposalStatus::Open,
            "Proposal is not open for voting"
        );

        // Check voting window hasn't expired
        let now = self.blockchain().get_block_timestamp();
        require!(
            now <= proposal.created_at + VOTING_PERIOD,
            "Voting period has expired"
        );

        let user_shares = self.shares(&caller).get();
        require!(user_shares > 0u64, "No voting power");

        let direction = if support {
            proposal.yes_votes += &user_shares;
            VoteDirection::Yes
        } else {
            proposal.no_votes += &user_shares;
            VoteDirection::No
        };

        // Record the vote for potential rage-quit accounting
        let vote_record = VoteRecord {
            voter: caller.clone(),
            direction,
            weight: user_shares.clone(),
        };
        self.vote_records(proposal_id).push(&vote_record);
        self.has_voted(proposal_id, &caller).set(true);
        self.agent_votes(&caller).push(&proposal_id);
        self.proposals(proposal_id).set(&proposal);

        self.vote_event(proposal_id, &caller, support, &user_shares);
    }

    // ========================================================
    // ENDPOINT: finalizeVoting
    // Called after voting window closes. Transitions Open → Passed or Failed.
    // ========================================================

    #[endpoint(finalizeVoting)]
    fn finalize_voting(&self, proposal_id: u64) {
        require!(
            !self.proposals(proposal_id).is_empty(),
            "Proposal does not exist"
        );

        let mut proposal = self.proposals(proposal_id).get();
        require!(
            proposal.status == ProposalStatus::Open,
            "Proposal is not open"
        );

        let now = self.blockchain().get_block_timestamp();
        require!(
            now > proposal.created_at + VOTING_PERIOD,
            "Voting period has not ended"
        );

        let effective_shares = self.voting_shares();
        let quorum_requirement = (&effective_shares * QUORUM_PERCENTAGE) / 100u64;

        if proposal.yes_votes >= quorum_requirement && proposal.yes_votes > proposal.no_votes {
            proposal.status = ProposalStatus::Passed;
            proposal.passed_at = now;
            self.proposals(proposal_id).set(&proposal);
            self.proposal_passed_event(proposal_id, now);
        } else {
            proposal.status = ProposalStatus::Failed;
            self.proposals(proposal_id).set(&proposal);
            self.proposal_failed_event(proposal_id);
        }
    }

    // ========================================================
    // ENDPOINT: executeProposal
    // Only after time-lock. Enforces epoch spending limit.
    // ========================================================

    #[endpoint(executeProposal)]
    fn execute_proposal(&self, proposal_id: u64) {
        let caller = self.blockchain().get_caller();
        require!(
            self.members().contains(&caller),
            "Only members can execute"
        );
        require!(
            !self.proposals(proposal_id).is_empty(),
            "Proposal does not exist"
        );

        let mut proposal = self.proposals(proposal_id).get();

        // Allow execution from Passed (auto-transition) or Executable
        if proposal.status == ProposalStatus::Passed {
            let now = self.blockchain().get_block_timestamp();
            require!(
                now > proposal.passed_at + TIMELOCK_PERIOD,
                "Time-lock period has not elapsed"
            );

            // Re-verify quorum still holds after potential rage-quits
            let effective_shares = self.voting_shares();
            let quorum_requirement = (&effective_shares * QUORUM_PERCENTAGE) / 100u64;
            if proposal.yes_votes < quorum_requirement || proposal.yes_votes <= proposal.no_votes {
                proposal.status = ProposalStatus::Failed;
                self.proposals(proposal_id).set(&proposal);
                self.proposal_failed_event(proposal_id);
                sc_panic!("Quorum lost after rage-quit withdrawals");
            }

            proposal.status = ProposalStatus::Executable;
        }

        require!(
            proposal.status == ProposalStatus::Executable,
            "Proposal is not executable"
        );

        // ── Guardrail: epoch spending limit ──
        let current_epoch = self.blockchain().get_block_epoch();
        let current_aum = self
            .blockchain()
            .get_sc_balance(&EgldOrEsdtTokenIdentifier::egld(), 0);
        let epoch_limit = (&current_aum * MAX_EPOCH_SPEND_BPS) / BPS_DENOMINATOR;
        let already_spent = self.epoch_spent(current_epoch).get();
        require!(
            &already_spent + &proposal.amount <= epoch_limit,
            "Epoch spending limit reached (25% of AUM)"
        );

        // Verify sufficient balance
        require!(
            current_aum >= proposal.amount,
            "Insufficient fund balance"
        );

        // Execute
        proposal.status = ProposalStatus::Executed;
        self.proposals(proposal_id).set(&proposal);
        self.epoch_spent(current_epoch)
            .update(|spent| *spent += &proposal.amount);

        self.send().direct_egld(&proposal.receiver, &proposal.amount);
        self.proposal_executed_event(proposal_id, &proposal.receiver, &proposal.amount);
    }

    // ========================================================
    // ENDPOINT: cancelProposal
    // Proposer can cancel their own proposal if still Open.
    // ========================================================

    #[endpoint(cancelProposal)]
    fn cancel_proposal(&self, proposal_id: u64) {
        let caller = self.blockchain().get_caller();
        require!(
            !self.proposals(proposal_id).is_empty(),
            "Proposal does not exist"
        );

        let mut proposal = self.proposals(proposal_id).get();
        require!(
            proposal.proposer == caller,
            "Only proposer can cancel"
        );
        require!(
            proposal.status == ProposalStatus::Open,
            "Can only cancel open proposals"
        );

        proposal.status = ProposalStatus::Cancelled;
        self.proposals(proposal_id).set(&proposal);

        self.proposal_cancelled_event(proposal_id, &caller);
    }

    // ========================================================
    // ENDPOINT: expireProposal
    // Anyone can call to transition an expired Open proposal to Failed.
    // Saves gas vs finalizeVoting when there's no quorum.
    // ========================================================

    #[endpoint(expireProposal)]
    fn expire_proposal(&self, proposal_id: u64) {
        require!(
            !self.proposals(proposal_id).is_empty(),
            "Proposal does not exist"
        );

        let mut proposal = self.proposals(proposal_id).get();
        require!(
            proposal.status == ProposalStatus::Open,
            "Proposal is not open"
        );

        let now = self.blockchain().get_block_timestamp();
        require!(
            now > proposal.created_at + VOTING_PERIOD,
            "Voting period has not ended"
        );

        proposal.status = ProposalStatus::Failed;
        self.proposals(proposal_id).set(&proposal);
        self.proposal_failed_event(proposal_id);
    }

    // ========================================================
    // INTERNAL: voting shares (excludes dead shares)
    // Dead shares exist only to prevent inflation attacks and
    // carry no voting power — exclude from quorum denominator.
    // ========================================================

    fn voting_shares(&self) -> BigUint {
        let total = self.total_shares().get();
        let dead = BigUint::from(DEAD_SHARES);
        if total > dead {
            total - dead
        } else {
            BigUint::zero()
        }
    }

    // ========================================================
    // INTERNAL: rage-quit processing
    // When an agent withdraws, remove their vote weight from
    // any Open or Passed proposals with active windows.
    // ========================================================

    fn process_rage_quit(&self, agent: &ManagedAddress) {
        let now = self.blockchain().get_block_timestamp();
        let vote_list_len = self.agent_votes(agent).len();

        // If agent has no tracked votes (voted before upgrade),
        // fall back to scanning all proposals.
        if vote_list_len == 0 {
            self.process_rage_quit_legacy(agent, now);
            return;
        }

        for idx in 1..=vote_list_len {
            let proposal_id = self.agent_votes(agent).get(idx);

            if self.proposals(proposal_id).is_empty() {
                continue;
            }

            let mut proposal = self.proposals(proposal_id).get();

            // Process Open and Passed proposals only
            match proposal.status {
                ProposalStatus::Open => {
                    // Only if voting window is still active
                    if now > proposal.created_at + VOTING_PERIOD {
                        continue;
                    }
                }
                ProposalStatus::Passed => {
                    // Only if still within time-lock window
                    if now > proposal.passed_at + TIMELOCK_PERIOD {
                        continue;
                    }
                }
                _ => {
                    continue;
                }
            }

            if !self.has_voted(proposal_id, agent).get() {
                continue;
            }

            self.remove_vote_weight(&mut proposal, proposal_id, agent);
            self.proposals(proposal_id).set(&proposal);
            self.rage_quit_event(proposal_id, agent);
        }
    }

    /// Legacy fallback for agents who voted before the upgrade
    /// (their agent_votes mapper is empty). Scans all proposals.
    fn process_rage_quit_legacy(&self, agent: &ManagedAddress, now: u64) {
        let count = self.proposal_count().get();

        for proposal_id in 1..=count {
            if self.proposals(proposal_id).is_empty() {
                continue;
            }

            let mut proposal = self.proposals(proposal_id).get();

            match proposal.status {
                ProposalStatus::Open => {
                    if now > proposal.created_at + VOTING_PERIOD {
                        continue;
                    }
                }
                ProposalStatus::Passed => {
                    if now > proposal.passed_at + TIMELOCK_PERIOD {
                        continue;
                    }
                }
                _ => {
                    continue;
                }
            }

            if !self.has_voted(proposal_id, agent).get() {
                continue;
            }

            self.remove_vote_weight(&mut proposal, proposal_id, agent);
            self.proposals(proposal_id).set(&proposal);
            self.rage_quit_event(proposal_id, agent);
        }
    }

    /// Finds an agent's vote record on a proposal and subtracts their weight.
    fn remove_vote_weight(
        &self,
        proposal: &mut Proposal<Self::Api>,
        proposal_id: u64,
        agent: &ManagedAddress,
    ) {
        let vote_count = self.vote_records(proposal_id).len();
        for i in 1..=vote_count {
            let record = self.vote_records(proposal_id).get(i);
            if record.voter == *agent {
                match record.direction {
                    VoteDirection::Yes => {
                        if proposal.yes_votes >= record.weight {
                            proposal.yes_votes -= &record.weight;
                        } else {
                            proposal.yes_votes = BigUint::zero();
                        }
                    }
                    VoteDirection::No => {
                        if proposal.no_votes >= record.weight {
                            proposal.no_votes -= &record.weight;
                        } else {
                            proposal.no_votes = BigUint::zero();
                        }
                    }
                }
                break;
            }
        }
    }

    // ========================================================
    // VIEWS — read-only queries
    // ========================================================

    #[view(getProposal)]
    fn get_proposal(&self, id: u64) -> Proposal<Self::Api> {
        self.proposals(id).get()
    }

    #[view(getProposals)]
    fn get_proposals(&self, from: u64, count: u64) -> MultiValueEncoded<Proposal<Self::Api>> {
        let mut result = MultiValueEncoded::new();
        if count == 0 {
            return result;
        }
        let total = self.proposal_count().get();
        if total == 0 {
            return result;
        }
        let start = if from == 0 { 1u64 } else { from };
        if start > total {
            return result;
        }
        let end = core::cmp::min(start.saturating_add(count - 1), total);

        for i in start..=end {
            if !self.proposals(i).is_empty() {
                result.push(self.proposals(i).get());
            }
        }
        result
    }

    #[view(getActiveProposals)]
    fn get_active_proposals(&self) -> MultiValueEncoded<Proposal<Self::Api>> {
        let mut result = MultiValueEncoded::new();
        let total = self.proposal_count().get();
        let now = self.blockchain().get_block_timestamp();

        for i in 1..=total {
            if self.proposals(i).is_empty() {
                continue;
            }
            let proposal = self.proposals(i).get();
            match proposal.status {
                ProposalStatus::Open => {
                    // Only include if voting window hasn't expired
                    if now <= proposal.created_at + VOTING_PERIOD {
                        result.push(proposal);
                    }
                }
                ProposalStatus::Passed | ProposalStatus::Executable => {
                    result.push(proposal);
                }
                _ => {}
            }
        }
        result
    }

    #[view(getFundStats)]
    fn get_fund_stats(&self) -> MultiValue5<BigUint, BigUint, u64, u64, u64> {
        let aum = self
            .blockchain()
            .get_sc_balance(&EgldOrEsdtTokenIdentifier::egld(), 0);
        let shares = self.total_shares().get();
        let member_count = self.members().len() as u64;
        let proposal_count = self.proposal_count().get();
        let min_uptime = self.min_uptime_score().get();
        (aum, shares, member_count, proposal_count, min_uptime).into()
    }

    #[view(getSharePrice)]
    fn get_share_price(&self) -> BigUint {
        let total_shares = self.total_shares().get();
        if total_shares == 0u64 {
            return BigUint::from(10u64.pow(18)); // 1 CLAW = 1 share initially
        }
        let current_aum = self
            .blockchain()
            .get_sc_balance(&EgldOrEsdtTokenIdentifier::egld(), 0);
        (current_aum * BigUint::from(10u64.pow(18))) / total_shares
    }

    #[view(getMembers)]
    fn get_members(&self, from: u64, count: u64) -> MultiValueEncoded<ManagedAddress> {
        let mut result = MultiValueEncoded::new();
        let total = self.members().len();
        let start = from as usize;
        let end = core::cmp::min(start + count as usize, total);

        for (idx, member) in self.members().iter().enumerate() {
            if idx >= start && idx < end {
                result.push(member);
            }
            if idx >= end {
                break;
            }
        }
        result
    }

    #[view(getMemberShares)]
    fn get_member_shares(&self, agent: &ManagedAddress) -> BigUint {
        self.shares(agent).get()
    }

    #[view(getEpochSpent)]
    fn get_epoch_spent(&self, epoch: u64) -> BigUint {
        self.epoch_spent(epoch).get()
    }

    #[view(getVoteRecords)]
    fn get_vote_records(&self, proposal_id: u64) -> MultiValueEncoded<VoteRecord<Self::Api>> {
        let mut result = MultiValueEncoded::new();
        let count = self.vote_records(proposal_id).len();
        for i in 1..=count {
            result.push(self.vote_records(proposal_id).get(i));
        }
        result
    }

    #[view(hasAgentVoted)]
    fn has_agent_voted(&self, proposal_id: u64, agent: &ManagedAddress) -> bool {
        self.has_voted(proposal_id, agent).get()
    }

    #[view(getContractConfig)]
    fn get_contract_config(&self) -> MultiValue4<BigUint, u64, u64, u64> {
        let min_dep = self.min_deposit().get();
        let min_up = self.min_uptime_score().get();
        (min_dep, min_up, VOTING_PERIOD, TIMELOCK_PERIOD).into()
    }

    // ========================================================
    // EVENTS
    // ========================================================

    #[event("deposit")]
    fn deposit_event(
        &self,
        #[indexed] agent: &ManagedAddress,
        #[indexed] amount: &BigUint,
        shares: &BigUint,
    );

    #[event("withdraw")]
    fn withdraw_event(
        &self,
        #[indexed] agent: &ManagedAddress,
        #[indexed] amount: &BigUint,
        shares: &BigUint,
    );

    #[event("proposalCreated")]
    fn proposal_created_event(
        &self,
        #[indexed] proposal_id: u64,
        #[indexed] proposer: &ManagedAddress,
        #[indexed] bulletin_post_id: u64,
        timestamp: u64,
    );

    #[event("vote")]
    fn vote_event(
        &self,
        #[indexed] proposal_id: u64,
        #[indexed] voter: &ManagedAddress,
        #[indexed] support: bool,
        weight: &BigUint,
    );

    #[event("proposalPassed")]
    fn proposal_passed_event(
        &self,
        #[indexed] proposal_id: u64,
        #[indexed] passed_at: u64,
    );

    #[event("proposalFailed")]
    fn proposal_failed_event(&self, #[indexed] proposal_id: u64);

    #[event("proposalExecuted")]
    fn proposal_executed_event(
        &self,
        #[indexed] proposal_id: u64,
        #[indexed] receiver: &ManagedAddress,
        amount: &BigUint,
    );

    #[event("proposalCancelled")]
    fn proposal_cancelled_event(
        &self,
        #[indexed] proposal_id: u64,
        #[indexed] proposer: &ManagedAddress,
    );

    #[event("rageQuit")]
    fn rage_quit_event(
        &self,
        #[indexed] proposal_id: u64,
        #[indexed] agent: &ManagedAddress,
    );

    // ========================================================
    // STORAGE
    // ========================================================

    // ── Configuration ──

    #[storage_mapper("bondRegistryAddress")]
    fn bond_registry_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[storage_mapper("uptimeAddress")]
    fn uptime_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[storage_mapper("minDeposit")]
    fn min_deposit(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("minUptimeScore")]
    fn min_uptime_score(&self) -> SingleValueMapper<u64>;

    // ── Fund state ──

    #[storage_mapper("totalShares")]
    fn total_shares(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("shares")]
    fn shares(&self, agent: &ManagedAddress) -> SingleValueMapper<BigUint>;

    #[storage_mapper("members")]
    fn members(&self) -> UnorderedSetMapper<ManagedAddress>;

    // ── Proposals ──

    #[storage_mapper("proposalCount")]
    fn proposal_count(&self) -> SingleValueMapper<u64>;

    #[storage_mapper("proposals")]
    fn proposals(&self, id: u64) -> SingleValueMapper<Proposal<Self::Api>>;

    #[storage_mapper("voteRecords")]
    fn vote_records(&self, proposal_id: u64) -> VecMapper<VoteRecord<Self::Api>>;

    #[storage_mapper("hasVoted")]
    fn has_voted(&self, proposal_id: u64, voter: &ManagedAddress) -> SingleValueMapper<bool>;

    // ── Spending limits ──

    #[storage_mapper("epochSpent")]
    fn epoch_spent(&self, epoch: u64) -> SingleValueMapper<BigUint>;

    // ── Per-agent vote tracking for efficient rage-quit ──

    #[storage_mapper("agentVotes")]
    fn agent_votes(&self, agent: &ManagedAddress) -> VecMapper<u64>;
}
