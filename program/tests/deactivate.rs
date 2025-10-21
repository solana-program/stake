#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    crate::helpers::add_sysvars,
    helpers::{
        create_vote_account, initialize_stake_account, parse_stake_account,
        process_instruction_after_testing_missing_signers,
    },
    mollusk_svm::{result::Check, Mollusk},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        error::StakeError,
        instruction as ixn,
        state::{Authorized, Lockup, StakeStateV2},
    },
    solana_stake_program::{get_minimum_delegation, id},
    test_case::test_case,
};

fn mollusk_bpf() -> Mollusk {
    Mollusk::new(&id(), "solana_stake_program")
}

#[test_case(false; "activating")]
#[test_case(true; "active")]
fn test_deactivate(activate: bool) {
    let mut mollusk = mollusk_bpf();

    let minimum_delegation = get_minimum_delegation();

    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    let vote_account = Pubkey::new_unique();
    let vote_account_data = create_vote_account();

    let stake = Pubkey::new_unique();
    let mut stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        helpers::STAKE_RENT_EXEMPTION + minimum_delegation,
        &Authorized { staker, withdrawer },
        &Lockup::default(),
    );

    // Deactivating an undelegated account fails
    let instruction = ixn::deactivate_stake(&stake, &staker);
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );

    // Delegate
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account);
    let accounts = vec![
        (stake, stake_account.clone()),
        (vote_account, vote_account_data),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);
    stake_account = result.resulting_accounts[0].1.clone().into();

    if activate {
        // Advance epoch to activate
        let current_slot = mollusk.sysvars.clock.slot;
        let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;
        mollusk.warp_to_slot(current_slot + slots_per_epoch);
    }

    // Deactivate with withdrawer fails
    let instruction = ixn::deactivate_stake(&stake, &withdrawer);
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Deactivate succeeds
    let instruction = ixn::deactivate_stake(&stake, &staker);
    let accounts = vec![(stake, stake_account.clone())];

    let result = process_instruction_after_testing_missing_signers(
        &mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(helpers::STAKE_RENT_EXEMPTION + minimum_delegation)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let clock = mollusk.sysvars.clock.clone();
    let (_, stake_data, _) = parse_stake_account(&stake_account);
    assert_eq!(
        stake_data.unwrap().delegation.deactivation_epoch,
        clock.epoch
    );

    // Deactivate again fails
    let instruction = ixn::deactivate_stake(&stake, &staker);
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::AlreadyDeactivated.into())],
    );

    // Advance epoch
    let current_slot = mollusk.sysvars.clock.slot;
    let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;
    mollusk.warp_to_slot(current_slot + slots_per_epoch);

    // Deactivate again still fails
    let instruction = ixn::deactivate_stake(&stake, &staker);
    let accounts = vec![(stake, stake_account)];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::AlreadyDeactivated.into())],
    );
}
