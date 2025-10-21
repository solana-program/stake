#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{add_sysvars, StakeLifecycle, StakeTestContext},
    mollusk_svm::result::Check,
    solana_account::{AccountSharedData, WritableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{instruction as ixn, state::StakeStateV2},
    solana_stake_program::id,
    test_case::test_case,
};

#[test_case(StakeLifecycle::Uninitialized; "uninitialized")]
#[test_case(StakeLifecycle::Initialized; "initialized")]
#[test_case(StakeLifecycle::Activating; "activating")]
#[test_case(StakeLifecycle::Active; "active")]
#[test_case(StakeLifecycle::Deactivating; "deactivating")]
#[test_case(StakeLifecycle::Deactive; "deactive")]
#[test_case(StakeLifecycle::Closed; "closed")]
fn test_withdraw_stake(withdraw_source_type: StakeLifecycle) {
    let mut ctx = StakeTestContext::new();
    let staked_amount = ctx.minimum_delegation;
    let wallet_rent_exempt_reserve = Rent::default().minimum_balance(0);

    // Create source stake account at the specified lifecycle stage
    let (withdraw_source, mut withdraw_source_account) =
        ctx.create_stake_account(withdraw_source_type, staked_amount);

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
        ctx.withdrawer
    };

    // Withdraw that would end rent-exemption always fails
    let rent_spillover = if withdraw_source_type == StakeLifecycle::Closed {
        ctx.rent_exempt_reserve - Rent::default().minimum_balance(0) + 1
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

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);

    // For initialized/delegated accounts, the program itself checks and fails with InsufficientFunds
    // For uninitialized/closed accounts, the program succeeds but leaves accounts below rent exemption
    if withdraw_source_type == StakeLifecycle::Uninitialized
        || withdraw_source_type == StakeLifecycle::Closed
    {
        // Expect program success but rent check should fail - catch the panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.mollusk.process_and_validate_instruction(
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
        ctx.mollusk.process_and_validate_instruction(
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

        let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
        ctx.mollusk.process_and_validate_instruction(
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

        let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
        ctx.mollusk.process_and_validate_instruction(
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
            &ctx.mollusk,
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
            &ctx.mollusk,
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
            staked_amount + ctx.rent_exempt_reserve,
            None,
        );
        let accounts = vec![
            (withdraw_source, withdraw_source_account),
            (recipient2, recipient2_account),
        ];

        helpers::process_instruction_after_testing_missing_signers(
            &ctx.mollusk,
            &instruction,
            &accounts,
            &[
                Check::success(),
                Check::account(&recipient2)
                    .lamports(staked_amount + ctx.rent_exempt_reserve + wallet_rent_exempt_reserve)
                    .build(),
            ],
        );
    }
}

#[test]
fn test_withdraw_from_rewards_pool() {
    let ctx = StakeTestContext::new();
    let staked_amount = ctx.minimum_delegation;

    // Create a rewards pool account
    let rewards_pool_address = Pubkey::new_unique();
    let rewards_pool_data = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve + staked_amount,
        &StakeStateV2::RewardsPool,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let recipient = Pubkey::new_unique();
    let recipient_account = AccountSharedData::default();

    let instruction = ixn::withdraw(
        &rewards_pool_address,
        &ctx.withdrawer,
        &recipient,
        staked_amount,
        None,
    );
    let accounts = vec![
        (rewards_pool_address, rewards_pool_data),
        (recipient, recipient_account),
    ];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}
