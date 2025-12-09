#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{context::StakeTestContext, lifecycle::StakeLifecycle},
    mollusk_svm::result::Check,
    solana_account::{AccountSharedData, ReadableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_client::{
        instructions::{InitializeBuilder, InitializeCheckedBuilder},
        types::{Authorized, Lockup},
        StakeStateAccount,
    },
    solana_stake_interface::state::StakeStateV2,
    solana_stake_program::id,
    test_case::test_case,
};

#[derive(Debug, Clone, Copy)]
enum InitializeVariant {
    Initialize,
    InitializeChecked,
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize(variant: InitializeVariant) {
    let mut ctx = StakeTestContext::new();

    let custodian = Pubkey::new_unique();

    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };

    // InitializeChecked always uses default lockup
    let lockup = match variant {
        InitializeVariant::Initialize => Lockup {
            epoch: 1,
            unix_timestamp: 0,
            custodian,
        },
        InitializeVariant::InitializeChecked => {
            let default = solana_stake_interface::state::Lockup::default();
            Lockup {
                epoch: default.epoch,
                unix_timestamp: default.unix_timestamp,
                custodian: default.custodian,
            }
        }
    };

    // Create an uninitialized stake account
    let (stake, stake_account) = ctx.stake_account(StakeLifecycle::Uninitialized).build();

    // Build instruction using Codama-generated builders
    let instruction = match variant {
        InitializeVariant::Initialize => InitializeBuilder::new()
            .stake(stake)
            .arg0(authorized.clone())
            .arg1(lockup.clone())
            .instruction(),
        InitializeVariant::InitializeChecked => InitializeCheckedBuilder::new()
            .stake(stake)
            .stake_authority(authorized.staker)
            .withdraw_authority(authorized.withdrawer)
            .instruction(),
    };

    // Process the instruction
    let result = ctx
        .process(instruction)
        .account(stake, stake_account)
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateAccount::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();

    // Check that we see what we expect
    let resulting_account: AccountSharedData = result.resulting_accounts[0].1.clone().into();
    let stake_state = StakeStateAccount::from_bytes(resulting_account.data()).unwrap();
    let meta = stake_state.meta().unwrap();
    assert_eq!(meta.authorized, authorized);
    assert_eq!(meta.rent_exempt_reserve, ctx.rent_exempt_reserve);
    assert_eq!(meta.lockup, lockup.clone());

    // Attempting to initialize an already initialized stake account should fail
    let instruction = match variant {
        InitializeVariant::Initialize => InitializeBuilder::new()
            .stake(stake)
            .arg0(authorized)
            .arg1(lockup.clone())
            .instruction(),
        InitializeVariant::InitializeChecked => InitializeCheckedBuilder::new()
            .stake(stake)
            .stake_authority(ctx.staker)
            .withdraw_authority(ctx.withdrawer)
            .instruction(),
    };

    ctx.process(instruction)
        .account(stake, resulting_account)
        .checks(&[Check::err(ProgramError::InvalidAccountData)])
        .test_missing_signers(false)
        .execute();
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_insufficient_funds(variant: InitializeVariant) {
    let ctx = StakeTestContext::new();

    let custodian = Pubkey::new_unique();
    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };
    let lockup = match variant {
        InitializeVariant::Initialize => Lockup {
            epoch: 1,
            unix_timestamp: 0,
            custodian,
        },
        InitializeVariant::InitializeChecked => Lockup {
            epoch: 0,
            unix_timestamp: 0,
            custodian: Pubkey::default(),
        },
    };

    // Create account with insufficient lamports
    let stake_account = Pubkey::new_unique();
    let stake_account_data = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve / 2, // Not enough lamports
        &StakeStateV2::Uninitialized,
        StakeStateAccount::size_of(),
        &id(),
    )
    .unwrap();

    let instruction = match variant {
        InitializeVariant::Initialize => InitializeBuilder::new()
            .stake(stake_account)
            .arg0(authorized)
            .arg1(lockup)
            .instruction(),
        InitializeVariant::InitializeChecked => InitializeCheckedBuilder::new()
            .stake(stake_account)
            .stake_authority(ctx.staker)
            .withdraw_authority(ctx.withdrawer)
            .instruction(),
    };

    ctx.process(instruction)
        .account(stake_account, stake_account_data)
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .test_missing_signers(false)
        .execute();
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_incorrect_size_larger(variant: InitializeVariant) {
    let ctx = StakeTestContext::new();

    // Original program_test.rs uses double rent instead of just
    // increasing the size by 1. This behavior remains (makes no difference here).
    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateAccount::size_of() * 2);

    let custodian = Pubkey::new_unique();
    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };
    let lockup = match variant {
        InitializeVariant::Initialize => Lockup {
            epoch: 1,
            unix_timestamp: 0,
            custodian,
        },
        InitializeVariant::InitializeChecked => Lockup {
            epoch: 0,
            unix_timestamp: 0,
            custodian: Pubkey::default(),
        },
    };

    // Create account with wrong size
    let stake_account = Pubkey::new_unique();
    let stake_account_data = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateAccount::size_of() + 1, // Too large
        &id(),
    )
    .unwrap();

    let instruction = match variant {
        InitializeVariant::Initialize => InitializeBuilder::new()
            .stake(stake_account)
            .arg0(authorized)
            .arg1(lockup)
            .instruction(),
        InitializeVariant::InitializeChecked => InitializeCheckedBuilder::new()
            .stake(stake_account)
            .stake_authority(ctx.staker)
            .withdraw_authority(ctx.withdrawer)
            .instruction(),
    };

    ctx.process(instruction)
        .account(stake_account, stake_account_data)
        .checks(&[Check::err(ProgramError::InvalidAccountData)])
        .test_missing_signers(false)
        .execute();
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_incorrect_size_smaller(variant: InitializeVariant) {
    let ctx = StakeTestContext::new();

    // Original program_test.rs uses rent for size instead of
    // rent for size - 1. This behavior remains (makes no difference here).
    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateAccount::size_of());

    let custodian = Pubkey::new_unique();
    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };
    let lockup = match variant {
        InitializeVariant::Initialize => Lockup {
            epoch: 1,
            unix_timestamp: 0,
            custodian,
        },
        InitializeVariant::InitializeChecked => Lockup {
            epoch: 0,
            unix_timestamp: 0,
            custodian: Pubkey::default(),
        },
    };

    // Create account with wrong size
    let stake_account = Pubkey::new_unique();
    let stake_account_data = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateAccount::size_of() - 1, // Too small
        &id(),
    )
    .unwrap();

    let instruction = match variant {
        InitializeVariant::Initialize => InitializeBuilder::new()
            .stake(stake_account)
            .arg0(authorized)
            .arg1(lockup)
            .instruction(),
        InitializeVariant::InitializeChecked => InitializeCheckedBuilder::new()
            .stake(stake_account)
            .stake_authority(ctx.staker)
            .withdraw_authority(ctx.withdrawer)
            .instruction(),
    };

    ctx.process(instruction)
        .account(stake_account, stake_account_data)
        .checks(&[Check::err(ProgramError::InvalidAccountData)])
        .test_missing_signers(false)
        .execute();
}
