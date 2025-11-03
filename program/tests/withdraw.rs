#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        context::StakeTestContext, instruction_builders::WithdrawConfig, lifecycle::StakeLifecycle,
    },
    mollusk_svm::result::Check,
    solana_account::{AccountSharedData, WritableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::state::StakeStateV2,
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
    let (withdraw_source, mut withdraw_source_account) = ctx
        .stake_account(withdraw_source_type)
        .staked_amount(staked_amount.unwrap())
        .build();

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

    // For initialized/delegated accounts, the program itself checks and fails with InsufficientFunds
    // For uninitialized/closed accounts, the program succeeds but leaves accounts below rent exemption
    if withdraw_source_type == StakeLifecycle::Uninitialized
        || withdraw_source_type == StakeLifecycle::Closed
    {
        // Expect program success but rent check should fail - catch the panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.process_with(WithdrawConfig {
                stake: (&withdraw_source, &withdraw_source_account),
                override_signer: Some(&signer),
                recipient: (&recipient, &recipient_account),
                amount: staked_amount.unwrap() + rent_spillover,
            })
            .checks(&[Check::success(), Check::all_rent_exempt()])
            .execute()
        }));
        // The rent exemption check should panic
        assert!(
            result.is_err(),
            "Expected rent exemption check to fail for uninitialized/closed withdraw"
        );
    } else {
        // Program fails with InsufficientFunds
        ctx.process_with(WithdrawConfig {
            stake: (&withdraw_source, &withdraw_source_account),
            override_signer: Some(&signer),
            recipient: (&recipient, &recipient_account),
            amount: staked_amount.unwrap() + rent_spillover,
        })
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .execute();
    }

    if withdraw_source_type.withdraw_minimum_enforced() {
        // Withdraw active or activating stake fails
        ctx.process_with(WithdrawConfig {
            stake: (&withdraw_source, &withdraw_source_account),
            override_signer: Some(&signer),
            recipient: (&recipient, &recipient_account),
            amount: staked_amount.unwrap(),
        })
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .execute();

        // Grant rewards
        let reward_amount = 10;
        withdraw_source_account
            .checked_add_lamports(reward_amount)
            .unwrap();

        // Withdraw in excess of rewards is not allowed
        ctx.process_with(WithdrawConfig {
            stake: (&withdraw_source, &withdraw_source_account),
            override_signer: Some(&signer),
            recipient: (&recipient, &recipient_account),
            amount: reward_amount + 1,
        })
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .execute();

        // Withdraw rewards is allowed
        ctx.process_with(WithdrawConfig {
            stake: (&withdraw_source, &withdraw_source_account),
            override_signer: Some(&signer),
            recipient: (&recipient, &recipient_account),
            amount: reward_amount,
        })
        .checks(&[
            Check::success(),
            Check::account(&recipient)
                .lamports(reward_amount + wallet_rent_exempt_reserve)
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    } else {
        // Withdraw that leaves rent behind is allowed
        let result = ctx
            .process_with(WithdrawConfig {
                stake: (&withdraw_source, &withdraw_source_account),
                override_signer: Some(&signer),
                recipient: (&recipient, &recipient_account),
                amount: staked_amount.unwrap(),
            })
            .checks(&[
                Check::success(),
                Check::account(&recipient)
                    .lamports(staked_amount.unwrap() + wallet_rent_exempt_reserve)
                    .build(),
            ])
            .test_missing_signers(true)
            .execute();

        withdraw_source_account = result.resulting_accounts[0].1.clone().into();

        // Full withdraw is allowed (add back staked_amount)
        withdraw_source_account
            .checked_add_lamports(staked_amount.unwrap())
            .unwrap();

        let recipient2 = Pubkey::new_unique();
        let mut recipient2_account = AccountSharedData::default();
        recipient2_account.set_lamports(wallet_rent_exempt_reserve);

        ctx.process_with(WithdrawConfig {
            stake: (&withdraw_source, &withdraw_source_account),
            override_signer: Some(&signer),
            recipient: (&recipient2, &recipient2_account),
            amount: staked_amount.unwrap() + ctx.rent_exempt_reserve,
        })
        .checks(&[
            Check::success(),
            Check::account(&recipient2)
                .lamports(
                    staked_amount.unwrap() + ctx.rent_exempt_reserve + wallet_rent_exempt_reserve,
                )
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    }
}

#[test]
fn test_withdraw_from_rewards_pool() {
    let ctx = StakeTestContext::new();
    let staked_amount = ctx.minimum_delegation;

    // Create a rewards pool account
    let rewards_pool_address = Pubkey::new_unique();
    let rewards_pool_data = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve + staked_amount.unwrap(),
        &StakeStateV2::RewardsPool,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let recipient = Pubkey::new_unique();
    let recipient_account = AccountSharedData::default();

    // Withdraw from program-owned non-stake accounts is not allowed
    ctx.process_with(WithdrawConfig {
        stake: (&rewards_pool_address, &rewards_pool_data),
        recipient: (&recipient, &recipient_account),
        amount: staked_amount.unwrap(),
        override_signer: None,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .test_missing_signers(false)
    .execute();
}
