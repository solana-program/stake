#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        AuthorizeCheckedConfig, AuthorizeCheckedWithSeedConfig, InitializeCheckedConfig,
        SetLockupCheckedConfig, StakeTestContext,
    },
    mollusk_svm::result::Check,
    solana_pubkey::Pubkey,
    solana_sdk_ids::system_program,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, StakeAuthorize},
    },
};

#[test]
fn test_initialize_checked() {
    let mut ctx = StakeTestContext::new();

    let (stake, stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Uninitialized)
        .build();

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
    let mut ctx = StakeTestContext::new();

    let new_authority = Pubkey::new_unique();

    let (stake, initialized_stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Initialized)
        .build();

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
    let mut ctx = StakeTestContext::new();

    let new_authority = Pubkey::new_unique();

    let (stake, initialized_stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Initialized)
        .build();

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
    let mut ctx = StakeTestContext::new();

    let seed_base = Pubkey::new_unique();
    let seed = "test seed";
    let seeded_address = Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();
    let new_authority = Pubkey::new_unique();

    let (stake, initialized_stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Initialized)
        .stake_authority(&seeded_address)
        .withdraw_authority(&seeded_address)
        .build();

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
    let mut ctx = StakeTestContext::new();

    let seed_base = Pubkey::new_unique();
    let seed = "test seed";
    let seeded_address = Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();
    let new_authority = Pubkey::new_unique();

    let (stake, initialized_stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Initialized)
        .stake_authority(&seeded_address)
        .withdraw_authority(&seeded_address)
        .build();

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
    let mut ctx = StakeTestContext::new();

    let custodian = Pubkey::new_unique();

    let (stake, initialized_stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Initialized)
        .build();

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
