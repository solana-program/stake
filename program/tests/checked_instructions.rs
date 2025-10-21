#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        initialize_stake_account, process_instruction_after_testing_missing_signers,
        StakeTestContext,
    },
    solana_account::AccountSharedData,
    solana_pubkey::Pubkey,
    solana_sdk_ids::system_program,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup, StakeAuthorize, StakeStateV2},
    },
    solana_stake_program::id,
};

#[test]
fn test_initialize_checked() {
    let ctx = StakeTestContext::new();

    let stake = Pubkey::new_unique();
    let instruction = ixn::initialize_checked(
        &stake,
        &Authorized {
            staker: ctx.staker,
            withdrawer: ctx.withdrawer,
        },
    );

    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let accounts = vec![(stake, stake_account)];

    process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &accounts,
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_staker() {
    let ctx = StakeTestContext::new();

    let stake = Pubkey::new_unique();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &ctx.mollusk,
        &stake,
        ctx.rent_exempt_reserve,
        &Authorized {
            staker: ctx.staker,
            withdrawer: ctx.withdrawer,
        },
        &Lockup::default(),
    );

    // Now test authorize checked
    let instruction = ixn::authorize_checked(
        &stake,
        &ctx.staker,
        &new_authority,
        StakeAuthorize::Staker,
        None,
    );

    process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_withdrawer() {
    let ctx = StakeTestContext::new();

    let stake = Pubkey::new_unique();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &ctx.mollusk,
        &stake,
        ctx.rent_exempt_reserve,
        &Authorized {
            staker: ctx.staker,
            withdrawer: ctx.withdrawer,
        },
        &Lockup::default(),
    );

    // Now test authorize checked
    let instruction = ixn::authorize_checked(
        &stake,
        &ctx.withdrawer,
        &new_authority,
        StakeAuthorize::Withdrawer,
        None,
    );

    process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_with_seed_staker() {
    let ctx = StakeTestContext::new();

    let stake = Pubkey::new_unique();
    let seed_base = Pubkey::new_unique();
    let seed = "test seed";
    let seeded_address = Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &ctx.mollusk,
        &stake,
        ctx.rent_exempt_reserve,
        &Authorized {
            staker: seeded_address,
            withdrawer: seeded_address,
        },
        &Lockup::default(),
    );

    // Now test authorize checked with seed
    let instruction = ixn::authorize_checked_with_seed(
        &stake,
        &seed_base,
        seed.to_string(),
        &system_program::id(),
        &new_authority,
        StakeAuthorize::Staker,
        None,
    );

    process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_with_seed_withdrawer() {
    let ctx = StakeTestContext::new();

    let stake = Pubkey::new_unique();
    let seed_base = Pubkey::new_unique();
    let seed = "test seed";
    let seeded_address = Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &ctx.mollusk,
        &stake,
        ctx.rent_exempt_reserve,
        &Authorized {
            staker: seeded_address,
            withdrawer: seeded_address,
        },
        &Lockup::default(),
    );

    // Now test authorize checked with seed
    let instruction = ixn::authorize_checked_with_seed(
        &stake,
        &seed_base,
        seed.to_string(),
        &system_program::id(),
        &new_authority,
        StakeAuthorize::Withdrawer,
        None,
    );

    process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_set_lockup_checked() {
    let ctx = StakeTestContext::new();

    let stake = Pubkey::new_unique();
    let custodian = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &ctx.mollusk,
        &stake,
        ctx.rent_exempt_reserve,
        &Authorized {
            staker: ctx.staker,
            withdrawer: ctx.withdrawer,
        },
        &Lockup::default(),
    );

    // Now test set lockup checked
    let instruction = ixn::set_lockup_checked(
        &stake,
        &ixn::LockupArgs {
            unix_timestamp: None,
            epoch: Some(1),
            custodian: Some(custodian),
        },
        &ctx.withdrawer,
    );

    process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}
