#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    crate::helpers::add_sysvars,
    helpers::{
        parse_stake_account, process_instruction_after_testing_missing_signers, StakeTestContext,
    },
    mollusk_svm::result::Check,
    solana_program_error::ProgramError,
    solana_stake_interface::{error::StakeError, instruction as ixn, state::StakeStateV2},
    solana_stake_program::id,
    test_case::test_case,
};

#[test_case(false; "activating")]
#[test_case(true; "active")]
fn test_deactivate(activate: bool) {
    let mut ctx = StakeTestContext::new();

    let (stake, mut stake_account) =
        ctx.create_stake_account(helpers::StakeLifecycle::Initialized, ctx.minimum_delegation);

    // Deactivating an undelegated account fails
    let instruction = ixn::deactivate_stake(&stake, &ctx.staker);
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );

    // Delegate
    let instruction = ixn::delegate_stake(&stake, &ctx.staker, &ctx.vote_account);
    let accounts = vec![
        (stake, stake_account.clone()),
        (ctx.vote_account, ctx.vote_account_data),
    ];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    let result =
        ctx.mollusk
            .process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);
    stake_account = result.resulting_accounts[0].1.clone().into();

    if activate {
        // Advance epoch to activate
        let current_slot = ctx.mollusk.sysvars.clock.slot;
        let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
        ctx.mollusk.warp_to_slot(current_slot + slots_per_epoch);
    }

    // Deactivate with withdrawer fails
    let instruction = ixn::deactivate_stake(&stake, &ctx.withdrawer);
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Deactivate succeeds
    let instruction = ixn::deactivate_stake(&stake, &ctx.staker);
    let accounts = vec![(stake, stake_account.clone())];

    let result = process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve + ctx.minimum_delegation)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let clock = ctx.mollusk.sysvars.clock.clone();
    let (_, stake_data, _) = parse_stake_account(&stake_account);
    assert_eq!(
        stake_data.unwrap().delegation.deactivation_epoch,
        clock.epoch
    );

    // Deactivate again fails
    let instruction = ixn::deactivate_stake(&stake, &ctx.staker);
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::AlreadyDeactivated.into())],
    );

    // Advance epoch
    let current_slot = ctx.mollusk.sysvars.clock.slot;
    let slots_per_epoch = ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch;
    ctx.mollusk.warp_to_slot(current_slot + slots_per_epoch);

    // Deactivate again still fails
    let instruction = ixn::deactivate_stake(&stake, &ctx.staker);
    let accounts = vec![(stake, stake_account)];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(StakeError::AlreadyDeactivated.into())],
    );
}
