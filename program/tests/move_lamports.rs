#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    crate::helpers::add_sysvars,
    helpers::{
        create_vote_account, get_effective_stake, parse_stake_account,
        true_up_transient_stake_epoch, StakeLifecycle,
    },
    mollusk_svm::{result::Check, Mollusk},
    solana_account::WritableAccount,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::{error::StakeError, instruction as ixn, state::Lockup},
    solana_stake_program::{get_minimum_delegation, id},
    test_case::test_matrix,
};

fn mollusk_bpf() -> Mollusk {
    Mollusk::new(&id(), "solana_stake_program")
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [false, true],
    [false, true]
)]
fn test_move_lamports(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    different_votes: bool,
    has_lockup: bool,
) {
    let mut mollusk = mollusk_bpf();

    let rent_exempt_reserve = helpers::STAKE_RENT_EXEMPTION;
    let minimum_delegation = get_minimum_delegation();

    // Put minimum in both accounts if they're active
    let source_staked_amount = if move_source_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    let dest_staked_amount = if move_dest_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    // Test with and without lockup
    let lockup = if has_lockup {
        let clock = mollusk.sysvars.clock.clone();
        Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 100,
            custodian: Pubkey::new_unique(),
        }
    } else {
        Lockup::default()
    };

    // We put an extra minimum in every account, unstaked, to test moving them
    let source_excess = minimum_delegation;
    let dest_excess = minimum_delegation;

    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    // Source vote account
    let source_vote_account = Pubkey::new_unique();
    let source_vote_account_data = create_vote_account();

    // Dest vote account (possibly different)
    let dest_vote_account = if different_votes {
        Pubkey::new_unique()
    } else {
        source_vote_account
    };
    let dest_vote_account_data = create_vote_account();

    // Create source stake (always with minimum_delegation, like original test)
    let move_source = Pubkey::new_unique();
    let mut move_source_account = move_source_type.create_stake_account_fully_specified(
        &mut mollusk,
        &source_vote_account,
        minimum_delegation,
        &staker,
        &withdrawer,
        &lockup,
    );

    // Create dest stake (always with minimum_delegation, like original test)
    let move_dest = Pubkey::new_unique();
    let mut move_dest_account = move_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &dest_vote_account,
        minimum_delegation,
        &staker,
        &withdrawer,
        &lockup,
    );

    // True up source epoch if transient (like original test)
    // This ensures both stakes are in the current epoch context
    true_up_transient_stake_epoch(
        &mut mollusk,
        &mut move_source_account,
        move_source_type,
        minimum_delegation,
        false,
    );

    // Add excess lamports if Active (like original test)
    if move_source_type == StakeLifecycle::Active {
        move_source_account
            .checked_add_lamports(source_excess)
            .unwrap();
    }
    if move_dest_type == StakeLifecycle::Active {
        move_dest_account.checked_add_lamports(dest_excess).unwrap();
    }

    // Clear out state failures (activating/deactivating not allowed)
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
        || move_dest_type == StakeLifecycle::Deactivating
    {
        let instruction = ixn::move_lamports(&move_source, &move_dest, &staker, source_excess);

        let mut accounts = vec![
            (move_source, move_source_account),
            (move_dest, move_dest_account),
            (source_vote_account, source_vote_account_data.clone()),
        ];
        if different_votes {
            accounts.push((dest_vote_account, dest_vote_account_data.clone()));
        }

        let accounts = add_sysvars(&mollusk, &instruction, accounts);
        let result = mollusk.process_instruction(&instruction, &accounts);
        assert!(result.program_result.is_err());
        return;
    }

    // Overshoot and fail for underfunded source
    let instruction = ixn::move_lamports(&move_source, &move_dest, &staker, source_excess + 1);

    let mut accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest, move_dest_account.clone()),
        (source_vote_account, source_vote_account_data.clone()),
    ];
    if different_votes {
        accounts.push((dest_vote_account, dest_vote_account_data.clone()));
    }

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidArgument)],
    );

    let before_source_lamports = parse_stake_account(&move_source_account).2;
    let before_dest_lamports = parse_stake_account(&move_dest_account).2;

    // Now properly move the full excess
    let instruction = ixn::move_lamports(&move_source, &move_dest, &staker, source_excess);

    let mut accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest, move_dest_account.clone()),
        (source_vote_account, source_vote_account_data),
    ];
    if different_votes {
        accounts.push((dest_vote_account, dest_vote_account_data));
    }

    let result = helpers::process_instruction_after_testing_missing_signers(
        &mollusk,
        &instruction,
        &accounts,
        &[Check::success()],
    );

    move_source_account = result.resulting_accounts[0].1.clone().into();
    move_dest_account = result.resulting_accounts[1].1.clone().into();

    let after_source_lamports = parse_stake_account(&move_source_account).2;
    let source_effective_stake = get_effective_stake(&mollusk, &move_source_account);

    // Source activation didn't change
    assert_eq!(source_effective_stake, source_staked_amount);

    // Source lamports are right
    assert_eq!(
        after_source_lamports,
        before_source_lamports - minimum_delegation
    );
    assert_eq!(
        after_source_lamports,
        source_effective_stake + rent_exempt_reserve
    );

    let after_dest_lamports = parse_stake_account(&move_dest_account).2;
    let dest_effective_stake = get_effective_stake(&mollusk, &move_dest_account);

    // Dest activation didn't change
    assert_eq!(dest_effective_stake, dest_staked_amount);

    // Dest lamports are right
    assert_eq!(
        after_dest_lamports,
        before_dest_lamports + minimum_delegation
    );
    assert_eq!(
        after_dest_lamports,
        dest_effective_stake + rent_exempt_reserve + source_excess + dest_excess
    );
}

#[test_matrix(
    [(StakeLifecycle::Active, StakeLifecycle::Uninitialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Initialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Uninitialized)]
)]
fn test_move_lamports_uninitialized_fail(move_types: (StakeLifecycle, StakeLifecycle)) {
    let mut mollusk = mollusk_bpf();

    let minimum_delegation = get_minimum_delegation();
    let source_staked_amount = minimum_delegation * 2;

    let (move_source_type, move_dest_type) = move_types;

    let vote_account = Pubkey::new_unique();
    let vote_account_data = create_vote_account();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    let move_source = Pubkey::new_unique();
    let move_source_account = move_source_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        source_staked_amount,
        &staker,
        &withdrawer,
        &Lockup::default(),
    );

    let move_dest = Pubkey::new_unique();
    let move_dest_account = move_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        0,
        &staker,
        &withdrawer,
        &Lockup::default(),
    );

    let source_signer = if move_source_type == StakeLifecycle::Uninitialized {
        move_source
    } else {
        staker
    };

    let instruction =
        ixn::move_lamports(&move_source, &move_dest, &source_signer, minimum_delegation);

    let accounts = vec![
        (move_source, move_source_account),
        (move_dest, move_dest_account),
        (vote_account, vote_account_data),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active, StakeLifecycle::Deactive]
)]
fn test_move_lamports_general_fail(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
) {
    let mut mollusk = mollusk_bpf();

    let minimum_delegation = get_minimum_delegation();
    let source_staked_amount = minimum_delegation * 2;

    let in_force_lockup = {
        let clock = mollusk.sysvars.clock.clone();
        Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 1_000_000,
            custodian: Pubkey::new_unique(),
        }
    };

    let vote_account = Pubkey::new_unique();
    let vote_account_data = create_vote_account();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    // Create source
    let move_source = Pubkey::new_unique();
    let mut move_source_account = move_source_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        source_staked_amount,
        &staker,
        &withdrawer,
        &Lockup::default(),
    );
    move_source_account
        .checked_add_lamports(minimum_delegation)
        .unwrap();

    // Self-move fails
    let instruction = ixn::move_lamports(&move_source, &move_source, &staker, minimum_delegation);
    let accounts = vec![
        (move_source, move_source_account.clone()),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidInstructionData)],
    );

    // Zero move fails
    // Create all dest types with minimum_delegation (like original test)
    let move_dest = Pubkey::new_unique();
    let mut move_dest_account = move_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        minimum_delegation,
        &staker,
        &withdrawer,
        &Lockup::default(),
    );

    // True up dest epoch if transient
    true_up_transient_stake_epoch(
        &mut mollusk,
        &mut move_dest_account,
        move_dest_type,
        minimum_delegation,
        false,
    );

    let instruction = ixn::move_lamports(&move_source, &move_dest, &staker, 0);
    let accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest, move_dest_account.clone()),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidArgument)],
    );

    // Sign with withdrawer fails
    let instruction = ixn::move_lamports(&move_source, &move_dest, &withdrawer, minimum_delegation);
    let accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest, move_dest_account),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Source lockup fails
    let move_locked_source = Pubkey::new_unique();
    let mut move_locked_source_account = move_source_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        source_staked_amount,
        &staker,
        &withdrawer,
        &in_force_lockup,
    );
    move_locked_source_account
        .checked_add_lamports(minimum_delegation)
        .unwrap();

    let move_dest2 = Pubkey::new_unique();
    let move_dest2_account = move_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        minimum_delegation,
        &staker,
        &withdrawer,
        &Lockup::default(),
    );

    let instruction = ixn::move_lamports(
        &move_locked_source,
        &move_dest2,
        &staker,
        minimum_delegation,
    );
    let accounts = vec![
        (move_locked_source, move_locked_source_account),
        (move_dest2, move_dest2_account),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::MergeMismatch.into())],
    );

    // Staker mismatch fails
    let throwaway_staker = Pubkey::new_unique();
    let move_dest3 = Pubkey::new_unique();
    let move_dest3_account = move_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        minimum_delegation,
        &throwaway_staker,
        &withdrawer,
        &Lockup::default(),
    );

    let instruction = ixn::move_lamports(&move_source, &move_dest3, &staker, minimum_delegation);
    let accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest3, move_dest3_account.clone()),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    // Authority mismatch always returns MergeMismatch
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::MergeMismatch.into())],
    );

    let instruction = ixn::move_lamports(
        &move_source,
        &move_dest3,
        &throwaway_staker,
        minimum_delegation,
    );
    let accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest3, move_dest3_account),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Withdrawer mismatch fails
    let throwaway_withdrawer = Pubkey::new_unique();
    let move_dest4 = Pubkey::new_unique();
    let move_dest4_account = move_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        minimum_delegation,
        &staker,
        &throwaway_withdrawer,
        &Lockup::default(),
    );

    let instruction = ixn::move_lamports(&move_source, &move_dest4, &staker, minimum_delegation);
    let accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest4, move_dest4_account.clone()),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    // Authority mismatch always returns MergeMismatch
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::MergeMismatch.into())],
    );

    let instruction = ixn::move_lamports(
        &move_source,
        &move_dest4,
        &throwaway_withdrawer,
        minimum_delegation,
    );
    let accounts = vec![
        (move_source, move_source_account.clone()),
        (move_dest4, move_dest4_account),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Dest lockup fails
    let move_dest5 = Pubkey::new_unique();
    let move_dest5_account = move_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        minimum_delegation,
        &staker,
        &withdrawer,
        &in_force_lockup,
    );

    let instruction = ixn::move_lamports(&move_source, &move_dest5, &staker, minimum_delegation);
    let accounts = vec![
        (move_source, move_source_account),
        (move_dest5, move_dest5_account),
        (vote_account, vote_account_data),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    // Lockup mismatch always returns MergeMismatch
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::MergeMismatch.into())],
    );
}
