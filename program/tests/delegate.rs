#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        context::StakeTestContext,
        instruction_builders::{DeactivateConfig, DelegateConfig},
        lifecycle::StakeLifecycle,
        stake_tracker::MolluskStakeExt,
        utils::{create_vote_account, increment_vote_account_credits, parse_stake_account},
    },
    mollusk_svm::result::Check,
    solana_account::{AccountSharedData, WritableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        error::StakeError,
        state::{Delegation, Stake, StakeStateV2},
    },
    solana_stake_program::id,
};

#[test]
fn test_delegate() {
    let mut ctx = StakeTestContext::with_delegation();
    let vote_account = *ctx.vote_account.as_ref().unwrap();
    let mut vote_account_data = ctx.vote_account_data.as_ref().unwrap().clone();
    let min_delegation = ctx.minimum_delegation.unwrap();

    let vote_state_credits = 100u64;
    increment_vote_account_credits(&mut vote_account_data, 0, vote_state_credits);

    let (stake, mut stake_account) = ctx
        .stake_account(StakeLifecycle::Initialized)
        .staked_amount(min_delegation)
        .build();

    // Delegate stake
    let result = ctx
        .process_with(DelegateConfig {
            stake: (&stake, &stake_account),
            vote: (&vote_account, &vote_account_data),
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve + min_delegation)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    // Verify that delegate() looks right
    let clock = ctx.mollusk.sysvars.clock.clone();
    let (_, stake_data, _) = parse_stake_account(&stake_account);
    assert_eq!(
        stake_data.unwrap(),
        Stake {
            delegation: Delegation {
                voter_pubkey: vote_account,
                stake: min_delegation,
                activation_epoch: clock.epoch,
                deactivation_epoch: u64::MAX,
                ..Delegation::default()
            },
            credits_observed: vote_state_credits,
        }
    );

    // Advance epoch to activate the stake
    let activation_epoch = ctx.mollusk.sysvars.clock.epoch;
    ctx.tracker.as_mut().unwrap().track_delegation(
        &stake,
        min_delegation,
        activation_epoch,
        &vote_account,
    );

    let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
    let current_slot = ctx.mollusk.sysvars.clock.slot;
    ctx.mollusk.warp_to_slot_with_stake_tracking(
        ctx.tracker.as_ref().unwrap(),
        current_slot + slots_per_epoch,
        Some(0),
    );

    // Verify that delegate fails as stake is active and not deactivating
    ctx.process_with(DelegateConfig {
        stake: (&stake, &stake_account),
        vote: (&vote_account, ctx.vote_account_data.as_ref().unwrap()),
    })
    .checks(&[Check::err(StakeError::TooSoonToRedelegate.into())])
    .execute();

    // Deactivate
    let result = ctx
        .process_with(DeactivateConfig {
            stake: (&stake, &stake_account),
            override_signer: None,
        })
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    // Create second vote account
    let (vote_account2, vote_account2_data) = ctx.create_second_vote_account();

    // Verify that delegate to a different vote account fails during deactivation
    ctx.process_with(DelegateConfig {
        stake: (&stake, &stake_account),
        vote: (&vote_account2, &vote_account2_data),
    })
    .checks(&[Check::err(StakeError::TooSoonToRedelegate.into())])
    .execute();

    // Verify that delegate succeeds to same vote account when stake is deactivating
    let result = ctx
        .process_with(DelegateConfig {
            stake: (&stake, &stake_account),
            vote: (&vote_account, ctx.vote_account_data.as_ref().unwrap()),
        })
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    // Verify that deactivation has been cleared
    let (_, stake_data, _) = parse_stake_account(&stake_account);
    assert_eq!(stake_data.unwrap().delegation.deactivation_epoch, u64::MAX);

    // Verify that delegate to a different vote account fails if stake is still active
    ctx.process_with(DelegateConfig {
        stake: (&stake, &stake_account),
        vote: (&vote_account2, &vote_account2_data),
    })
    .checks(&[Check::err(StakeError::TooSoonToRedelegate.into())])
    .execute();

    // Advance epoch again using tracker
    let current_slot = ctx.mollusk.sysvars.clock.slot;
    let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
    ctx.mollusk.warp_to_slot_with_stake_tracking(
        ctx.tracker.as_ref().unwrap(),
        current_slot + slots_per_epoch,
        Some(0),
    );

    // Delegate still fails after stake is fully activated; redelegate is not supported
    let (vote_account2, vote_account2_data) = ctx.create_second_vote_account();

    ctx.process_with(DelegateConfig {
        stake: (&stake, &stake_account),
        vote: (&vote_account2, &vote_account2_data),
    })
    .checks(&[Check::err(StakeError::TooSoonToRedelegate.into())])
    .execute();
}

#[test]
fn test_delegate_fake_vote_account() {
    let mut ctx = StakeTestContext::with_delegation();

    // Create fake vote account (not owned by vote program)
    let fake_vote_account = Pubkey::new_unique();
    let mut fake_vote_data = create_vote_account();
    fake_vote_data.set_owner(Pubkey::new_unique()); // Wrong owner

    let min_delegation = ctx.minimum_delegation.unwrap();
    let (stake, stake_account) = ctx
        .stake_account(StakeLifecycle::Initialized)
        .staked_amount(min_delegation)
        .build();

    // Try to delegate to fake vote account
    ctx.process_with(DelegateConfig {
        stake: (&stake, &stake_account),
        vote: (&fake_vote_account, &fake_vote_data),
    })
    .checks(&[Check::err(ProgramError::IncorrectProgramId)])
    .test_missing_signers(false)
    .execute();
}

#[test]
fn test_delegate_non_stake_account() {
    let ctx = StakeTestContext::with_delegation();

    // Create a rewards pool account (program-owned but not a stake account)
    let rewards_pool = Pubkey::new_unique();
    let rewards_pool_data = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve,
        &StakeStateV2::RewardsPool,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    ctx.process_with(DelegateConfig {
        stake: (&rewards_pool, &rewards_pool_data),
        vote: (
            ctx.vote_account.as_ref().unwrap(),
            ctx.vote_account_data.as_ref().unwrap(),
        ),
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .test_missing_signers(false)
    .execute();
}
