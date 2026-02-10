multiversx_sc::imports!();
multiversx_sc::derive_imports!();

// ============================================================
// Proposal Status — lifecycle states
// ============================================================

#[type_abi]
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, Clone, PartialEq, Debug)]
pub enum ProposalStatus {
    /// Voting is open. Agents can vote yes/no.
    Open,
    /// Quorum reached, yes > no. Time-lock period active.
    Passed,
    /// Time-lock elapsed. Any member can trigger execution.
    Executable,
    /// Funds sent. Terminal state.
    Executed,
    /// Voting window expired without quorum, or no >= yes,
    /// or rage-quit during time-lock dropped votes below threshold.
    Failed,
    /// Proposer cancelled before execution.
    Cancelled,
}

// ============================================================
// Proposal — the core governance record
// ============================================================

#[type_abi]
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, Clone, Debug)]
pub struct Proposal<M: ManagedTypeApi> {
    pub id: u64,
    pub proposer: ManagedAddress<M>,
    pub description: ManagedBuffer<M>,
    pub receiver: ManagedAddress<M>,
    pub amount: BigUint<M>,
    pub status: ProposalStatus,
    pub yes_votes: BigUint<M>,
    pub no_votes: BigUint<M>,
    pub created_at: u64,
    /// Block timestamp when voting ended and time-lock started (0 if still Open)
    pub passed_at: u64,
    /// Bulletin Board post ID linking to the discussion thread
    pub bulletin_post_id: u64,
}

// ============================================================
// Vote Record — tracks individual agent votes for rage-quit
// ============================================================

#[type_abi]
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, Clone, PartialEq, Debug)]
pub enum VoteDirection {
    Yes,
    No,
}

#[type_abi]
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, Clone, Debug)]
pub struct VoteRecord<M: ManagedTypeApi> {
    pub voter: ManagedAddress<M>,
    pub direction: VoteDirection,
    pub weight: BigUint<M>,
}
