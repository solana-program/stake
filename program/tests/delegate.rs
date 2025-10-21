#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    crate::helpers::add_sysvars,
    helpers::{
        create_vote_account, increment_vote_account_credits, initialize_stake_account,
        parse_stake_account,
    },
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, WritableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        error::StakeError,
        instruction as ixn,
        state::{Authorized, Delegation, Lockup, Stake, StakeStateV2},
    },
    solana_stake_program::{get_minimum_delegation, id},
};

fn mollusk_bpf() -> Mollusk {
    Mollusk::new(&id(), "solana_stake_program")
}

#[test]
fn test_delegate() {
    let mut mollusk = mollusk_bpf();

    let minimum_delegation = get_minimum_delegation();
    let rent_exempt_reserve = helpers::STAKE_RENT_EXEMPTION;

    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    let vote_account = Pubkey::new_unique();
    let mut vote_account_data = create_vote_account();

    let vote_state_credits = 100u64;
    increment_vote_account_credits(&mut vote_account_data, 0, vote_state_credits);

    let stake = Pubkey::new_unique();
    let mut stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        rent_exempt_reserve + minimum_delegation,
        &Authorized { staker, withdrawer },
        &Lockup::default(),
    );

    // Delegate stake
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account);
    let accounts = vec![
        (stake, stake_account.clone()),
        (vote_account, vote_account_data.clone()),
    ];

    let result = helpers::process_instruction_after_testing_missing_signers(
        &mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(rent_exempt_reserve + minimum_delegation)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    // Verify that delegate() looks right
    let clock = mollusk.sysvars.clock.clone();
    let (_, stake_data, _) = parse_stake_account(&stake_account);
    assert_eq!(
        stake_data.unwrap(),
        Stake {
            delegation: Delegation {
                voter_pubkey: vote_account,
                stake: minimum_delegation,
                activation_epoch: clock.epoch,
                deactivation_epoch: u64::MAX,
                ..Delegation::default()
            },
            credits_observed: vote_state_credits,
        }
    );

    // Advance epoch to activate the stake
    let activation_epoch = mollusk.sysvars.clock.epoch;
    helpers::advance_epoch_and_activate_stake(&mut mollusk, minimum_delegation, activation_epoch);

    // Verify that delegate fails as stake is active and not deactivating
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account);
    let accounts = vec![
        (stake, stake_account.clone()),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::TooSoonToRedelegate.into())],
    );

    // Deactivate
    let instruction = ixn::deactivate_stake(&stake, &staker);
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);
    stake_account = result.resulting_accounts[0].1.clone().into();

    // Create second vote account
    let vote_account2 = Pubkey::new_unique();
    let vote_account2_data = create_vote_account();

    // Verify that delegate to a different vote account fails during deactivation
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2);
    let accounts = vec![
        (stake, stake_account.clone()),
        (vote_account2, vote_account2_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::TooSoonToRedelegate.into())],
    );

    // Verify that delegate succeeds to same vote account when stake is deactivating
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account);
    let accounts = vec![
        (stake, stake_account.clone()),
        (vote_account, vote_account_data.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);
    stake_account = result.resulting_accounts[0].1.clone().into();

    // Verify that deactivation has been cleared
    let (_, stake_data, _) = parse_stake_account(&stake_account);
    assert_eq!(stake_data.unwrap().delegation.deactivation_epoch, u64::MAX);

    // Verify that delegate to a different vote account fails if stake is still active
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2);
    let accounts = vec![
        (stake, stake_account.clone()),
        (vote_account2, vote_account2_data),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::TooSoonToRedelegate.into())],
    );

    // Advance epoch again (just warp forward, maintaining history continuity)
    let current_epoch = mollusk.sysvars.clock.epoch;
    let current_slot = mollusk.sysvars.clock.slot;
    let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;

    // Warp to next epoch first
    mollusk.warp_to_slot(current_slot + slots_per_epoch);

    // Now add history for the now-past epoch (current_epoch)
    // Carry forward all effective stake, consolidate any activating stake
    let mut stake_history = mollusk.sysvars.stake_history.clone();
    let prev_entry = stake_history.get(current_epoch).cloned();

    let total_effective = if let Some(entry) = prev_entry {
        entry.effective + entry.activating
    } else {
        // No entry for previous epoch; look further back
        if current_epoch > 0 {
            let earlier_entry = stake_history.get(current_epoch - 1).cloned();
            earlier_entry
                .map(|e| e.effective + e.activating)
                .unwrap_or(0)
        } else {
            0
        }
    };

    stake_history.add(
        current_epoch,
        solana_stake_interface::stake_history::StakeHistoryEntry {
            effective: total_effective,
            activating: 0,
            deactivating: 0,
        },
    );
    mollusk.sysvars.stake_history = stake_history;

    // Delegate still fails after stake is fully activated; redelegate is not supported
    let vote_account2 = Pubkey::new_unique();
    let vote_account2_data = create_vote_account();

    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2);
    let accounts = vec![
        (stake, stake_account.clone()),
        (vote_account2, vote_account2_data),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::TooSoonToRedelegate.into())],
    );
}

#[test]
fn test_delegate_fake_vote_account() {
    let mollusk = mollusk_bpf();

    let minimum_delegation = get_minimum_delegation();
    let rent_exempt_reserve = helpers::STAKE_RENT_EXEMPTION;

    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    // Create fake vote account (not owned by vote program)
    let fake_vote_account = Pubkey::new_unique();
    let mut fake_vote_data = create_vote_account();
    fake_vote_data.set_owner(Pubkey::new_unique()); // Wrong owner

    let stake = Pubkey::new_unique();
    let stake_account = initialize_stake_account(
        &mollusk,
        &stake,
        rent_exempt_reserve + minimum_delegation,
        &Authorized { staker, withdrawer },
        &Lockup::default(),
    );

    // Try to delegate to fake vote account
    let instruction = ixn::delegate_stake(&stake, &staker, &fake_vote_account);
    let accounts = vec![(stake, stake_account), (fake_vote_account, fake_vote_data)];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::IncorrectProgramId)],
    );
}

#[test]
fn test_delegate_non_stake_account() {
    let mollusk = mollusk_bpf();

    let staker = Pubkey::new_unique();
    let vote_account = Pubkey::new_unique();
    let vote_account_data = create_vote_account();

    // Create a rewards pool account (program-owned but not a stake account)
    let rewards_pool = Pubkey::new_unique();
    let rewards_pool_data = AccountSharedData::new_data_with_space(
        helpers::STAKE_RENT_EXEMPTION,
        &StakeStateV2::RewardsPool,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let instruction = ixn::delegate_stake(&rewards_pool, &staker, &vote_account);
    let accounts = vec![
        (rewards_pool, rewards_pool_data),
        (vote_account, vote_account_data),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}
