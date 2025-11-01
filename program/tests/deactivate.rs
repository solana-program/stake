#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        instruction_builders::{DeactivateConfig, DelegateConfig},
        utils::parse_stake_account,
        StakeTestContext,
    },
    mollusk_svm::result::Check,
    solana_program_error::ProgramError,
    solana_stake_interface::{error::StakeError, state::StakeStateV2},
    solana_stake_program::id,
    test_case::test_case,
};

#[test_case(false; "activating")]
#[test_case(true; "active")]
fn test_deactivate(activate: bool) {
    let mut ctx = StakeTestContext::new();
    let min_delegation = ctx.minimum_delegation;

    let (stake, mut stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Initialized)
        .staked_amount(min_delegation)
        .build();

    // Deactivating an undelegated account fails
    ctx.process_with(DeactivateConfig {
        stake: (&stake, &stake_account),
        override_signer: None,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .execute();

    // Delegate
    let result = ctx
        .process_with(DelegateConfig {
            stake: (&stake, &stake_account),
            vote: (&ctx.vote_account, &ctx.vote_account_data),
        })
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    if activate {
        // Advance epoch to activate
        let current_slot = ctx.mollusk.sysvars.clock.slot;
        let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
        ctx.mollusk.warp_to_slot(current_slot + slots_per_epoch);
    }

    // Deactivate with withdrawer fails
    ctx.process_with(DeactivateConfig {
        stake: (&stake, &stake_account),
        override_signer: Some(&ctx.withdrawer),
    })
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
    .execute();

    // Deactivate succeeds
    let result = ctx
        .process_with(DeactivateConfig {
            stake: (&stake, &stake_account),
            override_signer: None,
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve + ctx.minimum_delegation)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers()
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    let clock = ctx.mollusk.sysvars.clock.clone();
    let (_, stake_data, _) = parse_stake_account(&stake_account);
    assert_eq!(
        stake_data.unwrap().delegation.deactivation_epoch,
        clock.epoch
    );

    // Deactivate again fails
    ctx.process_with(DeactivateConfig {
        stake: (&stake, &stake_account),
        override_signer: None,
    })
    .checks(&[Check::err(StakeError::AlreadyDeactivated.into())])
    .execute();

    // Advance epoch
    let current_slot = ctx.mollusk.sysvars.clock.slot;
    let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
    ctx.mollusk.warp_to_slot(current_slot + slots_per_epoch);

    // Deactivate again still fails
    ctx.process_with(DeactivateConfig {
        stake: (&stake, &stake_account),
        override_signer: None,
    })
    .checks(&[Check::err(StakeError::AlreadyDeactivated.into())])
    .test_missing_signers()
    .execute();
}
