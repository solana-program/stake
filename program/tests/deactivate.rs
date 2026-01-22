#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{context::StakeTestContext, lifecycle::StakeLifecycle},
    mollusk_svm::result::Check,
    solana_account::ReadableAccount,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_client::instructions::{DeactivateBuilder, DelegateStakeBuilder},
    solana_stake_interface::{error::StakeError, state::StakeStateV2},
    solana_stake_program::id,
    test_case::test_case,
};

#[test_case(false; "activating")]
#[test_case(true; "active")]
fn test_deactivate(activate: bool) {
    let mut ctx = StakeTestContext::with_delegation();
    let min_delegation = ctx.minimum_delegation.unwrap();
    let rent_exempt_reserve = ctx.rent_exempt_reserve;
    let staker = ctx.staker;
    let withdrawer = ctx.withdrawer;

    let (stake, mut stake_account) = ctx
        .stake_account(StakeLifecycle::Initialized)
        .staked_amount(min_delegation)
        .build();

    let (vote, vote_account_data) = ctx.vote_account.clone().unwrap();

    // Deactivating an undelegated account fails
    ctx.checks(&[Check::err(ProgramError::InvalidAccountData)])
        .test_missing_signers(false)
        .execute(
            DeactivateBuilder::new()
                .stake(stake)
                .stake_authority(staker)
                .instruction(),
            &[(&stake, &stake_account)],
        );

    // Delegate
    let result = ctx.execute(
        DelegateStakeBuilder::new()
            .stake(stake)
            .vote(vote)
            .unused(Pubkey::new_unique())
            .stake_authority(staker)
            .instruction(),
        &[(&stake, &stake_account), (&vote, &vote_account_data)],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    if activate {
        // Advance epoch to activate
        let current_slot = ctx.mollusk.sysvars.clock.slot;
        let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
        ctx.mollusk.warp_to_slot(current_slot + slots_per_epoch);
    }

    // Deactivate with withdrawer fails
    ctx.checks(&[Check::err(ProgramError::MissingRequiredSignature)])
        .test_missing_signers(false)
        .execute(
            DeactivateBuilder::new()
                .stake(stake)
                .stake_authority(withdrawer)
                .instruction(),
            &[(&stake, &stake_account)],
        );

    // Deactivate succeeds
    let result = ctx
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(rent_exempt_reserve + min_delegation)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .execute(
            DeactivateBuilder::new()
                .stake(stake)
                .stake_authority(staker)
                .instruction(),
            &[(&stake, &stake_account)],
        );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let clock = ctx.mollusk.sysvars.clock.clone();
    let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();
    if let StakeStateV2::Stake(_, stake_data, _) = stake_state {
        assert_eq!(stake_data.delegation.deactivation_epoch, clock.epoch);
    } else {
        panic!("Expected StakeStateV2::Stake");
    }

    // Deactivate again fails
    ctx.checks(&[Check::err(StakeError::AlreadyDeactivated.into())])
        .test_missing_signers(false)
        .execute(
            DeactivateBuilder::new()
                .stake(stake)
                .stake_authority(staker)
                .instruction(),
            &[(&stake, &stake_account)],
        );

    // Advance epoch
    let current_slot = ctx.mollusk.sysvars.clock.slot;
    let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
    ctx.mollusk.warp_to_slot(current_slot + slots_per_epoch);

    // Deactivate again still fails
    ctx.checks(&[Check::err(StakeError::AlreadyDeactivated.into())])
        .test_missing_signers(false)
        .execute(
            DeactivateBuilder::new()
                .stake(stake)
                .stake_authority(staker)
                .instruction(),
            &[(&stake, &stake_account)],
        );
}
