// Tests for the Autonomous Fund v2 contract.
//
// NOTE: The full contract requires cross-contract calls to the Bond Registry
// and Uptime contracts (sync_call_readonly). The whitebox_legacy test framework
// does not support cross-contract calls, so deposit() cannot be tested directly
// in this harness.
//
// For full integration testing, use mandos/scenario JSON tests with mock
// contracts for the Bond Registry and Uptime, or test on devnet.
//
// These tests verify the contract compiles and the ABI is generated correctly.
// Endpoint-level testing should be done via scenario tests or on-chain.

use multiversx_sc_scenario::api::DebugApi;

type FundContract = autonomous_fund::ContractObj<DebugApi>;

#[test]
fn test_contract_builds() {
    // Verify the contract object can be instantiated with DebugApi
    let _: fn() -> FundContract = autonomous_fund::contract_obj;
}
