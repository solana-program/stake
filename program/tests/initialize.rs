#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{context::StakeTestContext, lifecycle::StakeLifecycle},
    mollusk_svm::result::Check,
    solana_account::{AccountSharedData, ReadableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_sdk_ids::{stake::id, system_program::id as system_program_id},
    solana_stake_client::instructions::{InitializeBuilder, InitializeCheckedBuilder},
    solana_stake_interface::state::StakeStateV2,
    test_case::test_case,
};

#[derive(Debug, Clone, Copy)]
enum InitializeVariant {
    Initialize,
    InitializeChecked,
}

fn lockup_for(
    variant: InitializeVariant,
    custodian: Pubkey,
) -> solana_stake_interface::state::Lockup {
    match variant {
        InitializeVariant::Initialize => solana_stake_interface::state::Lockup {
            epoch: 1,
            unix_timestamp: 0,
            custodian,
        },
        InitializeVariant::InitializeChecked => solana_stake_interface::state::Lockup::default(),
    }
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize(variant: InitializeVariant) {
    let mut ctx = StakeTestContext::default();

    let staker = ctx.staker;
    let withdrawer = ctx.withdrawer;
    let lockup = lockup_for(variant, Pubkey::new_unique());

    let (stake, stake_account) = ctx.stake_account(StakeLifecycle::Uninitialized).build();

    let result = {
        let program_id = id();
        let checks = [
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&program_id)
                .space(StakeStateV2::size_of())
                .build(),
        ];

        match variant {
            InitializeVariant::Initialize => ctx.checks(&checks).execute(
                InitializeBuilder::new()
                    .stake(stake)
                    .arg0(solana_stake_client::types::Authorized { staker, withdrawer })
                    .arg1(solana_stake_client::types::Lockup {
                        unix_timestamp: lockup.unix_timestamp,
                        epoch: lockup.epoch,
                        custodian: lockup.custodian,
                    })
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
            InitializeVariant::InitializeChecked => ctx.checks(&checks).execute(
                InitializeCheckedBuilder::new()
                    .stake(stake)
                    .stake_authority(staker)
                    .withdraw_authority(withdrawer)
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
        }
    };

    let resulting_account: AccountSharedData = result.resulting_accounts[0].1.clone().into();
    let stake_state: StakeStateV2 = bincode::deserialize(resulting_account.data()).unwrap();
    assert_eq!(
        stake_state,
        StakeStateV2::Initialized(solana_stake_interface::state::Meta {
            authorized: solana_stake_interface::state::Authorized { staker, withdrawer },
            rent_exempt_reserve: ctx.rent_exempt_reserve,
            lockup,
        }),
    );

    // Re-initialize should fail
    match variant {
        InitializeVariant::Initialize => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(
                InitializeBuilder::new()
                    .stake(stake)
                    .arg0(solana_stake_client::types::Authorized { staker, withdrawer })
                    .arg1(solana_stake_client::types::Lockup {
                        unix_timestamp: lockup.unix_timestamp,
                        epoch: lockup.epoch,
                        custodian: lockup.custodian,
                    })
                    .instruction(),
                &[(&stake, &resulting_account)],
            ),
        InitializeVariant::InitializeChecked => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(
                InitializeCheckedBuilder::new()
                    .stake(stake)
                    .stake_authority(staker)
                    .withdraw_authority(withdrawer)
                    .instruction(),
                &[(&stake, &resulting_account)],
            ),
    };
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_insufficient_funds(variant: InitializeVariant) {
    let mut ctx = StakeTestContext::default();

    let staker = ctx.staker;
    let withdrawer = ctx.withdrawer;
    let lockup = lockup_for(variant, Pubkey::new_unique());

    // Account has insufficient lamports
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve / 2,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    match variant {
        InitializeVariant::Initialize => ctx
            .checks(&[Check::err(ProgramError::InsufficientFunds)])
            .test_missing_signers(false)
            .execute(
                InitializeBuilder::new()
                    .stake(stake)
                    .arg0(solana_stake_client::types::Authorized { staker, withdrawer })
                    .arg1(solana_stake_client::types::Lockup {
                        unix_timestamp: lockup.unix_timestamp,
                        epoch: lockup.epoch,
                        custodian: lockup.custodian,
                    })
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
        InitializeVariant::InitializeChecked => ctx
            .checks(&[Check::err(ProgramError::InsufficientFunds)])
            .test_missing_signers(false)
            .execute(
                InitializeCheckedBuilder::new()
                    .stake(stake)
                    .stake_authority(staker)
                    .withdraw_authority(withdrawer)
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
    };
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_incorrect_size_larger(variant: InitializeVariant) {
    let mut ctx = StakeTestContext::default();

    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of() * 2);

    let staker = ctx.staker;
    let withdrawer = ctx.withdrawer;
    let lockup = lockup_for(variant, Pubkey::new_unique());

    // Account data length too large
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of() + 1,
        &id(),
    )
    .unwrap();

    match variant {
        InitializeVariant::Initialize => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(
                InitializeBuilder::new()
                    .stake(stake)
                    .arg0(solana_stake_client::types::Authorized { staker, withdrawer })
                    .arg1(solana_stake_client::types::Lockup {
                        unix_timestamp: lockup.unix_timestamp,
                        epoch: lockup.epoch,
                        custodian: lockup.custodian,
                    })
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
        InitializeVariant::InitializeChecked => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(
                InitializeCheckedBuilder::new()
                    .stake(stake)
                    .stake_authority(staker)
                    .withdraw_authority(withdrawer)
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
    };
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_incorrect_size_smaller(variant: InitializeVariant) {
    let mut ctx = StakeTestContext::default();

    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of());

    let staker = ctx.staker;
    let withdrawer = ctx.withdrawer;
    let lockup = lockup_for(variant, Pubkey::new_unique());

    // Account data length too small
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of() - 1,
        &id(),
    )
    .unwrap();

    match variant {
        InitializeVariant::Initialize => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(
                InitializeBuilder::new()
                    .stake(stake)
                    .arg0(solana_stake_client::types::Authorized { staker, withdrawer })
                    .arg1(solana_stake_client::types::Lockup {
                        unix_timestamp: lockup.unix_timestamp,
                        epoch: lockup.epoch,
                        custodian: lockup.custodian,
                    })
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
        InitializeVariant::InitializeChecked => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(
                InitializeCheckedBuilder::new()
                    .stake(stake)
                    .stake_authority(staker)
                    .withdraw_authority(withdrawer)
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
    };
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_wrong_owner(variant: InitializeVariant) {
    let mut ctx = StakeTestContext::default();

    let staker = ctx.staker;
    let withdrawer = ctx.withdrawer;
    let lockup = lockup_for(variant, Pubkey::new_unique());

    // Owner is not the stake program
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        Rent::default().minimum_balance(StakeStateV2::size_of()),
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &system_program_id(),
    )
    .unwrap();

    match variant {
        InitializeVariant::Initialize => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountOwner)])
            .test_missing_signers(false)
            .execute(
                InitializeBuilder::new()
                    .stake(stake)
                    .arg0(solana_stake_client::types::Authorized { staker, withdrawer })
                    .arg1(solana_stake_client::types::Lockup {
                        unix_timestamp: lockup.unix_timestamp,
                        epoch: lockup.epoch,
                        custodian: lockup.custodian,
                    })
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
        InitializeVariant::InitializeChecked => ctx
            .checks(&[Check::err(ProgramError::InvalidAccountOwner)])
            .test_missing_signers(false)
            .execute(
                InitializeCheckedBuilder::new()
                    .stake(stake)
                    .stake_authority(staker)
                    .withdraw_authority(withdrawer)
                    .instruction(),
                &[(&stake, &stake_account)],
            ),
    };
}
