#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        initialize_stake_account, process_instruction_after_testing_missing_signers,
        STAKE_RENT_EXEMPTION,
    },
    mollusk_svm::Mollusk,
    solana_account::AccountSharedData,
    solana_pubkey::Pubkey,
    solana_sdk_ids::system_program,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup, StakeAuthorize, StakeStateV2},
    },
    solana_stake_program::id,
};

fn mollusk_bpf() -> Mollusk {
    Mollusk::new(&id(), "solana_stake_program")
}

#[test]
fn test_initialize_checked() {
    let mollusk = mollusk_bpf();

    let stake = Pubkey::new_unique();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    let instruction = ixn::initialize_checked(&stake, &Authorized { staker, withdrawer });

    let stake_account = AccountSharedData::new_data_with_space(
        STAKE_RENT_EXEMPTION,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let accounts = vec![(stake, stake_account)];

    process_instruction_after_testing_missing_signers(
        &mollusk,
        &instruction,
        &accounts,
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_staker() {
    let mollusk = mollusk_bpf();

    let stake = Pubkey::new_unique();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        STAKE_RENT_EXEMPTION,
        &Authorized { staker, withdrawer },
        &Lockup::default(),
    );

    // Now test authorize checked
    let instruction = ixn::authorize_checked(
        &stake,
        &staker,
        &new_authority,
        StakeAuthorize::Staker,
        None,
    );

    process_instruction_after_testing_missing_signers(
        &mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_withdrawer() {
    let mollusk = mollusk_bpf();

    let stake = Pubkey::new_unique();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        STAKE_RENT_EXEMPTION,
        &Authorized { staker, withdrawer },
        &Lockup::default(),
    );

    // Now test authorize checked
    let instruction = ixn::authorize_checked(
        &stake,
        &withdrawer,
        &new_authority,
        StakeAuthorize::Withdrawer,
        None,
    );

    process_instruction_after_testing_missing_signers(
        &mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_with_seed_staker() {
    let mollusk = mollusk_bpf();

    let stake = Pubkey::new_unique();
    let seed_base = Pubkey::new_unique();
    let seed = "test seed";
    let seeded_address = Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        STAKE_RENT_EXEMPTION,
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
        &mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_authorize_checked_with_seed_withdrawer() {
    let mollusk = mollusk_bpf();

    let stake = Pubkey::new_unique();
    let seed_base = Pubkey::new_unique();
    let seed = "test seed";
    let seeded_address = Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();
    let new_authority = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        STAKE_RENT_EXEMPTION,
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
        &mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}

#[test]
fn test_set_lockup_checked() {
    let mollusk = mollusk_bpf();

    let stake = Pubkey::new_unique();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();
    let custodian = Pubkey::new_unique();

    let initialized_stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        STAKE_RENT_EXEMPTION,
        &Authorized { staker, withdrawer },
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
        &withdrawer,
    );

    process_instruction_after_testing_missing_signers(
        &mollusk,
        &instruction,
        &[(stake, initialized_stake_account)],
        &[mollusk_svm::result::Check::success()],
    );
}
