use multiversx_sc::types::Address;
use multiversx_sc_scenario::{
    api::DebugApi,
    managed_address, managed_biguint, rust_biguint, whitebox_legacy::*,
};

use autonomous_fund::*;

const WASM_PATH: &'static str = "output/autonomous-fund.wasm";

type FundContract = ContractObj<DebugApi>;

struct AutonomousFundSetup<ContractObjBuilder>
where
    ContractObjBuilder: 'static + Copy + Fn() -> FundContract,
{
    pub blockchain_wrapper: BlockchainStateWrapper,
    pub owner_address: Address,
    pub contract_address: Address,
    pub contract_wrapper: ContractObjWrapper<
        FundContract,
        ContractObjBuilder,
    >,
}

fn setup_fund<ContractObjBuilder>(
    contract_builder: ContractObjBuilder,
) -> AutonomousFundSetup<ContractObjBuilder>
where
    ContractObjBuilder: 'static + Copy + Fn() -> FundContract,
{
    let mut blockchain_wrapper = BlockchainStateWrapper::new();
    let owner_address = blockchain_wrapper.create_user_account(&rust_biguint!(0));
    let contract_wrapper = blockchain_wrapper.create_sc_account(
        &rust_biguint!(0),
        Some(&owner_address),
        contract_builder,
        WASM_PATH,
    );

    blockchain_wrapper
        .execute_tx(&owner_address, &contract_wrapper, &rust_biguint!(0), |sc: FundContract| {
            sc.init();
        })
        .assert_ok();

    AutonomousFundSetup {
        blockchain_wrapper,
        owner_address,
        contract_address: contract_wrapper.address_ref().clone(),
        contract_wrapper,
    }
}

#[test]
fn test_deposit() {
    let mut setup = setup_fund(autonomous_fund::contract_obj);
    let user = setup.blockchain_wrapper.create_user_account(&rust_biguint!(100));

    setup.blockchain_wrapper
        .execute_tx(&user, &setup.contract_wrapper, &rust_biguint!(10), |sc: FundContract| {
            sc.deposit();
        })
        .assert_ok();
        
    setup.blockchain_wrapper
        .execute_query(&setup.contract_wrapper, |sc: FundContract| {
            let shares = sc.shares(&managed_address!(&user)).get();
            assert!(shares > 0);
        })
        .assert_ok();
}

#[test]
fn test_proposal_flow() {
    let mut setup = setup_fund(autonomous_fund::contract_obj);
    let user = setup.blockchain_wrapper.create_user_account(&rust_biguint!(1000));
    let receiver = setup.blockchain_wrapper.create_user_account(&rust_biguint!(0));

    // 1. Deposit
    setup.blockchain_wrapper
        .execute_tx(&user, &setup.contract_wrapper, &rust_biguint!(100), |sc: FundContract| {
            sc.deposit();
        })
        .assert_ok();

    // 2. Submit Proposal
    setup.blockchain_wrapper
        .execute_tx(&user, &setup.contract_wrapper, &rust_biguint!(0), |sc: FundContract| {
            sc.submit_proposal(
                multiversx_sc::types::ManagedBuffer::from(b"Invest"),
                managed_address!(&receiver),
                managed_biguint!(50),
            );
        })
        .assert_ok();

    // 3. Vote
    setup.blockchain_wrapper
        .execute_tx(&user, &setup.contract_wrapper, &rust_biguint!(0), |sc: FundContract| {
            sc.vote(1);
        })
        .assert_ok();

    // 4. Execute
    setup.blockchain_wrapper
        .execute_tx(&user, &setup.contract_wrapper, &rust_biguint!(0), |sc: FundContract| {
            sc.execute_proposal(1);
        })
        .assert_ok();

    // Verify receiver got funds
    setup.blockchain_wrapper.check_egld_balance(&receiver, &rust_biguint!(50));
}
