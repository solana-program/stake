#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        initialize_stake_account, AuthorizeCheckedConfig, AuthorizeCheckedWithSeedConfig,
        InitializeCheckedConfig, SetLockupCheckedConfig, StakeTestContext,
    },
    mollusk_svm::result::Check,
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
    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    ctx.process_with(InitializeCheckedConfig {
        stake: (&stake, &stake_account),
        authorized: &Authorized {
            staker: ctx.staker,
            withdrawer: ctx.withdrawer,
        },
    })
    .checks(&[Check::success()])
    .test_missing_signers(true)
    .execute();
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
    ctx.process_with(AuthorizeCheckedConfig {
        stake: (&stake, &initialized_stake_account),
        authority: &ctx.staker,
        new_authority: &new_authority,
        stake_authorize: StakeAuthorize::Staker,
        custodian: None,
    })
    .checks(&[Check::success()])
    .test_missing_signers(true)
    .execute();
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
    ctx.process_with(AuthorizeCheckedConfig {
        stake: (&stake, &initialized_stake_account),
        authority: &ctx.withdrawer,
        new_authority: &new_authority,
        stake_authorize: StakeAuthorize::Withdrawer,
        custodian: None,
    })
    .checks(&[Check::success()])
    .test_missing_signers(true)
    .execute();
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
    ctx.process_with(AuthorizeCheckedWithSeedConfig {
        stake: (&stake, &initialized_stake_account),
        authority_base: &seed_base,
        authority_seed: seed.to_string(),
        authority_owner: &system_program::id(),
        new_authority: &new_authority,
        stake_authorize: StakeAuthorize::Staker,
        custodian: None,
    })
    .checks(&[Check::success()])
    .test_missing_signers(true)
    .execute();
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
    ctx.process_with(AuthorizeCheckedWithSeedConfig {
        stake: (&stake, &initialized_stake_account),
        authority_base: &seed_base,
        authority_seed: seed.to_string(),
        authority_owner: &system_program::id(),
        new_authority: &new_authority,
        stake_authorize: StakeAuthorize::Withdrawer,
        custodian: None,
    })
    .checks(&[Check::success()])
    .test_missing_signers(true)
    .execute();
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
    ctx.process_with(SetLockupCheckedConfig {
        stake: (&stake, &initialized_stake_account),
        lockup_args: &ixn::LockupArgs {
            unix_timestamp: None,
            epoch: Some(1),
            custodian: Some(custodian),
        },
        custodian: &ctx.withdrawer,
    })
    .checks(&[Check::success()])
    .test_missing_signers(true)
    .execute();
}
