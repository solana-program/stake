#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        context::StakeTestContext,
        instruction_builders::{InitializeCheckedConfig, InitializeConfig},
        lifecycle::StakeLifecycle,
        utils::add_sysvars,
    },
    mollusk_svm::result::Check,
    solana_account::{create_account_shared_data_for_test, AccountSharedData, ReadableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::{sysvar, Rent},
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup, StakeStateV2},
    },
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
        InitializeVariant::InitializeChecked => Lockup::default(),
    };

    // Create an uninitialized stake account
    let (stake, stake_account) = ctx.stake_account(StakeLifecycle::Uninitialized).build();

    // Process the Initialize instruction, including testing missing signers
    let result = match variant {
        InitializeVariant::Initialize => ctx
            .process_with(InitializeConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
                lockup: &lockup,
            })
            .checks(&[
                Check::success(),
                Check::all_rent_exempt(),
                Check::account(&stake)
                    .lamports(ctx.rent_exempt_reserve)
                    .owner(&id())
                    .space(StakeStateV2::size_of())
                    .build(),
            ])
            .test_missing_signers(true)
            .execute(),
        InitializeVariant::InitializeChecked => ctx
            .process_with(InitializeCheckedConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
            })
            .checks(&[
                Check::success(),
                Check::all_rent_exempt(),
                Check::account(&stake)
                    .lamports(ctx.rent_exempt_reserve)
                    .owner(&id())
                    .space(StakeStateV2::size_of())
                    .build(),
            ])
            .test_missing_signers(true)
            .execute(),
    };

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

    // Attempting to initialize an already initialized stake account should fail
    match variant {
        InitializeVariant::Initialize => ctx
            .process_with(InitializeConfig {
                stake: (&stake, &resulting_account),
                authorized: &authorized,
                lockup: &lockup,
            })
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(),
        InitializeVariant::InitializeChecked => ctx
            .process_with(InitializeCheckedConfig {
                stake: (&stake, &resulting_account),
                authorized: &authorized,
            })
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(),
    };
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
        InitializeVariant::InitializeChecked => Lockup::default(),
    };

    // Create account with insufficient lamports (need to manually create since builder adds rent automatically)
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve / 2, // Not enough lamports
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    match variant {
        InitializeVariant::Initialize => ctx
            .process_with(InitializeConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
                lockup: &lockup,
            })
            .checks(&[Check::err(ProgramError::InsufficientFunds)])
            .test_missing_signers(false)
            .execute(),
        InitializeVariant::InitializeChecked => ctx
            .process_with(InitializeCheckedConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
            })
            .checks(&[Check::err(ProgramError::InsufficientFunds)])
            .test_missing_signers(false)
            .execute(),
    };
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_incorrect_size_larger(variant: InitializeVariant) {
    let ctx = StakeTestContext::new();

    // Original program_test.rs uses double rent instead of just
    // increasing the size by 1. This behavior remains (makes no difference here).
    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of() * 2);

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
        InitializeVariant::InitializeChecked => Lockup::default(),
    };

    // Create account with wrong size (need to manually create since builder enforces correct size)
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of() + 1, // Too large
        &id(),
    )
    .unwrap();

    match variant {
        InitializeVariant::Initialize => ctx
            .process_with(InitializeConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
                lockup: &lockup,
            })
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(),
        InitializeVariant::InitializeChecked => ctx
            .process_with(InitializeCheckedConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
            })
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(),
    };
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_incorrect_size_smaller(variant: InitializeVariant) {
    let ctx = StakeTestContext::new();

    // Original program_test.rs uses rent for size instead of
    // rent for size - 1. This behavior remains (makes no difference here).
    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of());

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
        InitializeVariant::InitializeChecked => Lockup::default(),
    };

    // Create account with wrong size (need to manually create since builder enforces correct size)
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of() - 1, // Too small
        &id(),
    )
    .unwrap();

    match variant {
        InitializeVariant::Initialize => ctx
            .process_with(InitializeConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
                lockup: &lockup,
            })
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(),
        InitializeVariant::InitializeChecked => ctx
            .process_with(InitializeCheckedConfig {
                stake: (&stake, &stake_account),
                authorized: &authorized,
            })
            .checks(&[Check::err(ProgramError::InvalidAccountData)])
            .test_missing_signers(false)
            .execute(),
    };
}

/// Ensure that `initialize()` respects the minimum balance requirements
/// - Assert 1: accounts with a balance equal-to the rent exemption initialize OK
/// - Assert 2: accounts with a balance less-than the rent exemption do not initialize
#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_minimum_balance(variant: InitializeVariant) {
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
        InitializeVariant::InitializeChecked => Lockup::default(),
    };

    // Test exact rent boundary: success at rent_exempt_reserve, failure at rent_exempt_reserve - 1
    for (lamports, expected_result) in [
        (ctx.rent_exempt_reserve, Ok(())),
        (
            ctx.rent_exempt_reserve - 1,
            Err(ProgramError::InsufficientFunds),
        ),
    ] {
        let stake = Pubkey::new_unique();
        let stake_account = AccountSharedData::new_data_with_space(
            lamports,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();

        let check = match expected_result {
            Ok(()) => Check::success(),
            Err(e) => Check::err(e),
        };

        match variant {
            InitializeVariant::Initialize => ctx
                .process_with(InitializeConfig {
                    stake: (&stake, &stake_account),
                    authorized: &authorized,
                    lockup: &lockup,
                })
                .checks(&[check])
                .test_missing_signers(false)
                .execute(),
            InitializeVariant::InitializeChecked => ctx
                .process_with(InitializeCheckedConfig {
                    stake: (&stake, &stake_account),
                    authorized: &authorized,
                })
                .checks(&[check])
                .test_missing_signers(false)
                .execute(),
        };
    }
}

#[test_case(InitializeVariant::Initialize; "initialize")]
#[test_case(InitializeVariant::InitializeChecked; "initialize_checked")]
fn test_initialize_rent_change(variant: InitializeVariant) {
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
        InitializeVariant::InitializeChecked => Lockup::default(),
    };

    // Create account with sufficient lamports based on default rent
    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    // Build instruction
    let instruction = match variant {
        InitializeVariant::Initialize => ixn::initialize(&stake, &authorized, &lockup),
        InitializeVariant::InitializeChecked => ixn::initialize_checked(&stake, &authorized),
    };

    // Create modified rent with increased lamports_per_byte_year
    // This simulates rent increasing between account creation and initialization
    let default_rent = Rent::default();
    let modified_rent = Rent {
        lamports_per_byte_year: default_rent.lamports_per_byte_year + 1,
        ..default_rent
    };

    // Create rent sysvar account with modified rent
    let rent_account = create_account_shared_data_for_test(&modified_rent);

    // Include the modified rent account in the accounts list
    // add_sysvars will use this instead of the default when it sees rent::id() in instruction
    let accounts = vec![(stake, stake_account), (sysvar::id(), rent_account)];

    // Test that initialization fails with insufficient funds due to rent increase
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &add_sysvars(&ctx.mollusk, &instruction, accounts),
        &[Check::err(ProgramError::InsufficientFunds)],
    );
}
