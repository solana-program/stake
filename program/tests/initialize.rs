#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    crate::helpers::add_sysvars,
    helpers::StakeTestContext,
    mollusk_svm::result::Check,
    solana_account::{AccountSharedData, ReadableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup, StakeStateV2},
    },
    solana_stake_program::id,
};

#[test]
fn test_initialize() {
    let ctx = StakeTestContext::new();

    let custodian = Pubkey::new_unique();

    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };
    let lockup = Lockup {
        epoch: 1,
        unix_timestamp: 0,
        custodian,
    };

    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let accounts = vec![(stake, stake_account)];
    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);

    // Should succeed
    let result = ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );

    // Check that we see what we expect
    let resulting_account: AccountSharedData = result.resulting_accounts[0].1.clone().into();
    let stake_state: StakeStateV2 = bincode::deserialize(resulting_account.data()).unwrap();
    assert_eq!(
        stake_state,
        StakeStateV2::Initialized(solana_stake_interface::state::Meta {
            authorized,
            rent_exempt_reserve: ctx.rent_exempt_reserve,
            lockup,
        }),
    );

    // 2nd time fails, can't move it from anything other than uninit->init
    let accounts2 = add_sysvars(&ctx.mollusk, &instruction, vec![(stake, resulting_account)]);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts2,
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}

#[test]
fn test_initialize_insufficient_funds() {
    let ctx = StakeTestContext::new();

    let custodian = Pubkey::new_unique();
    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };
    let lockup = Lockup {
        epoch: 1,
        unix_timestamp: 0,
        custodian,
    };

    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve / 2, // Not enough lamports
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let accounts = vec![(stake, stake_account)];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InsufficientFunds)],
    );
}

#[test]
fn test_initialize_incorrect_size_larger() {
    let ctx = StakeTestContext::new();

    // Original program_test.rs uses double rent instead of just
    // increasing the size by 1. This behavior remains (makes no difference here).
    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of() * 2);

    let custodian = Pubkey::new_unique();
    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };
    let lockup = Lockup {
        epoch: 1,
        unix_timestamp: 0,
        custodian,
    };

    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of() + 1, // Too large
        &id(),
    )
    .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let accounts = vec![(stake, stake_account)];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}

#[test]
fn test_initialize_incorrect_size_smaller() {
    let ctx = StakeTestContext::new();

    // Original program_test.rs uses rent for size instead of
    // rent for size - 1. This behavior remains (makes no difference here).
    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of());

    let custodian = Pubkey::new_unique();
    let authorized = Authorized {
        staker: ctx.staker,
        withdrawer: ctx.withdrawer,
    };
    let lockup = Lockup {
        epoch: 1,
        unix_timestamp: 0,
        custodian,
    };

    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of() - 1, // Too small
        &id(),
    )
    .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let accounts = vec![(stake, stake_account)];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}
