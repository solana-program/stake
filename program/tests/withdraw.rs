#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{add_sysvars, StakeLifecycle},
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, WritableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{instruction as ixn, state::StakeStateV2},
    solana_stake_program::{get_minimum_delegation, id},
    test_case::test_case,
};

fn mollusk_bpf() -> Mollusk {
    Mollusk::new(&id(), "solana_stake_program")
}

#[test_case(StakeLifecycle::Uninitialized; "uninitialized")]
#[test_case(StakeLifecycle::Initialized; "initialized")]
#[test_case(StakeLifecycle::Activating; "activating")]
#[test_case(StakeLifecycle::Active; "active")]
#[test_case(StakeLifecycle::Deactivating; "deactivating")]
#[test_case(StakeLifecycle::Deactive; "deactive")]
#[test_case(StakeLifecycle::Closed; "closed")]
fn test_withdraw_stake(withdraw_source_type: StakeLifecycle) {
    let mut mollusk = mollusk_bpf();

    let stake_rent_exempt_reserve = helpers::STAKE_RENT_EXEMPTION;
    let minimum_delegation = get_minimum_delegation();
    let staked_amount = minimum_delegation;

    let wallet_rent_exempt_reserve = Rent::default().minimum_balance(0);

    let vote_account = Pubkey::new_unique();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    // Create source stake account at the specified lifecycle stage
    let withdraw_source = Pubkey::new_unique();
    let mut withdraw_source_account = withdraw_source_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        staked_amount,
        &staker,
        &withdrawer,
        &solana_stake_interface::state::Lockup::default(),
    );

    // Create recipient account
    let recipient = Pubkey::new_unique();
    let mut recipient_account = AccountSharedData::default();
    recipient_account.set_lamports(wallet_rent_exempt_reserve);

    // Determine signer based on lifecycle stage
    let signer = if withdraw_source_type == StakeLifecycle::Uninitialized
        || withdraw_source_type == StakeLifecycle::Closed
    {
        withdraw_source // Self-signed for uninitialized/closed
    } else {
        withdrawer
    };

    // Withdraw that would end rent-exemption always fails
    let rent_spillover = if withdraw_source_type == StakeLifecycle::Closed {
        stake_rent_exempt_reserve - Rent::default().minimum_balance(0) + 1
    } else {
        1
    };

    let instruction = ixn::withdraw(
        &withdraw_source,
        &signer,
        &recipient,
        staked_amount + rent_spillover,
        None,
    );
    let accounts = vec![
        (withdraw_source, withdraw_source_account.clone()),
        (recipient, recipient_account.clone()),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);

    // For initialized/delegated accounts, the program itself checks and fails with InsufficientFunds
    // For uninitialized/closed accounts, the program succeeds but leaves accounts below rent exemption
    if withdraw_source_type == StakeLifecycle::Uninitialized
        || withdraw_source_type == StakeLifecycle::Closed
    {
        // Expect program success but rent check should fail - catch the panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            mollusk.process_and_validate_instruction(
                &instruction,
                &accounts,
                &[Check::success(), Check::all_rent_exempt()],
            )
        }));
        // The rent exemption check should panic
        assert!(
            result.is_err(),
            "Expected rent exemption check to fail for uninitialized/closed withdraw"
        );
    } else {
        // Program fails with InsufficientFunds
        mollusk.process_and_validate_instruction(
            &instruction,
            &accounts,
            &[Check::err(ProgramError::InsufficientFunds)],
        );
    }

    if withdraw_source_type.withdraw_minimum_enforced() {
        // Withdraw active or activating stake fails
        let instruction = ixn::withdraw(&withdraw_source, &signer, &recipient, staked_amount, None);
        let accounts = vec![
            (withdraw_source, withdraw_source_account.clone()),
            (recipient, recipient_account.clone()),
        ];

        let accounts = add_sysvars(&mollusk, &instruction, accounts);
        mollusk.process_and_validate_instruction(
            &instruction,
            &accounts,
            &[Check::err(ProgramError::InsufficientFunds)],
        );

        // Grant rewards
        let reward_amount = 10;
        withdraw_source_account
            .checked_add_lamports(reward_amount)
            .unwrap();

        // Withdraw in excess of rewards is not allowed
        let instruction = ixn::withdraw(
            &withdraw_source,
            &signer,
            &recipient,
            reward_amount + 1,
            None,
        );
        let accounts = vec![
            (withdraw_source, withdraw_source_account.clone()),
            (recipient, recipient_account.clone()),
        ];

        let accounts = add_sysvars(&mollusk, &instruction, accounts);
        mollusk.process_and_validate_instruction(
            &instruction,
            &accounts,
            &[Check::err(ProgramError::InsufficientFunds)],
        );

        // Withdraw rewards is allowed
        let instruction = ixn::withdraw(&withdraw_source, &signer, &recipient, reward_amount, None);
        let accounts = vec![
            (withdraw_source, withdraw_source_account.clone()),
            (recipient, recipient_account.clone()),
        ];

        helpers::process_instruction_after_testing_missing_signers(
            &mollusk,
            &instruction,
            &accounts,
            &[
                Check::success(),
                Check::account(&recipient)
                    .lamports(reward_amount + wallet_rent_exempt_reserve)
                    .build(),
            ],
        );
    } else {
        // Withdraw that leaves rent behind is allowed
        let instruction = ixn::withdraw(&withdraw_source, &signer, &recipient, staked_amount, None);
        let accounts = vec![
            (withdraw_source, withdraw_source_account.clone()),
            (recipient, recipient_account.clone()),
        ];

        let result = helpers::process_instruction_after_testing_missing_signers(
            &mollusk,
            &instruction,
            &accounts,
            &[
                Check::success(),
                Check::account(&recipient)
                    .lamports(staked_amount + wallet_rent_exempt_reserve)
                    .build(),
            ],
        );

        withdraw_source_account = result.resulting_accounts[0].1.clone().into();

        // Full withdraw is allowed (add back staked_amount)
        withdraw_source_account
            .checked_add_lamports(staked_amount)
            .unwrap();

        let recipient2 = Pubkey::new_unique();
        let mut recipient2_account = AccountSharedData::default();
        recipient2_account.set_lamports(wallet_rent_exempt_reserve);

        let instruction = ixn::withdraw(
            &withdraw_source,
            &signer,
            &recipient2,
            staked_amount + stake_rent_exempt_reserve,
            None,
        );
        let accounts = vec![
            (withdraw_source, withdraw_source_account),
            (recipient2, recipient2_account),
        ];

        helpers::process_instruction_after_testing_missing_signers(
            &mollusk,
            &instruction,
            &accounts,
            &[
                Check::success(),
                Check::account(&recipient2)
                    .lamports(
                        staked_amount + stake_rent_exempt_reserve + wallet_rent_exempt_reserve,
                    )
                    .build(),
            ],
        );
    }
}

#[test]
fn test_withdraw_from_rewards_pool() {
    let mollusk = mollusk_bpf();

    let minimum_delegation = get_minimum_delegation();
    let staked_amount = minimum_delegation;

    let withdrawer = Pubkey::new_unique();

    // Create a rewards pool account
    let rewards_pool_address = Pubkey::new_unique();
    let rewards_pool_data = AccountSharedData::new_data_with_space(
        helpers::STAKE_RENT_EXEMPTION + staked_amount,
        &StakeStateV2::RewardsPool,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let recipient = Pubkey::new_unique();
    let recipient_account = AccountSharedData::default();

    let instruction = ixn::withdraw(
        &rewards_pool_address,
        &withdrawer,
        &recipient,
        staked_amount,
        None,
    );
    let accounts = vec![
        (rewards_pool_address, rewards_pool_data),
        (recipient, recipient_account),
    ];

    let accounts = add_sysvars(&mollusk, &instruction, accounts);
    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}
