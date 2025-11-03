#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        context::StakeTestContext,
        instruction_builders::{DeactivateConfig, DeactivateDelinquentConfig, DelegateConfig},
        lifecycle::StakeLifecycle,
        utils::{increment_credits, parse_stake_account},
    },
    mollusk_svm::result::Check,
    solana_account::AccountSharedData,
    solana_clock::Epoch,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{
        error::StakeError,
        stake_flags::StakeFlags,
        state::{Delegation, Meta, Stake, StakeStateV2},
        MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION,
    },
    solana_stake_program::id,
    solana_vote_interface::state::{VoteStateV4, VoteStateVersions},
    test_case::test_case,
};

#[test_case(false; "activating")]
#[test_case(true; "active")]
fn test_deactivate(activate: bool) {
    let mut ctx = StakeTestContext::with_delegation();
    let min_delegation = ctx.minimum_delegation.unwrap();

    let (stake, mut stake_account) = ctx
        .stake_account(StakeLifecycle::Initialized)
        .staked_amount(min_delegation)
        .build();

    // Deactivating an undelegated account fails
    ctx.process_with(DeactivateConfig {
        stake: (&stake, &stake_account),
        override_signer: None,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .test_missing_signers(false)
    .execute();

    // Delegate
    let result = ctx
        .process_with(DelegateConfig {
            stake: (&stake, &stake_account),
            vote: (
                ctx.vote_account.as_ref().unwrap(),
                ctx.vote_account_data.as_ref().unwrap(),
            ),
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
    .test_missing_signers(false)
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
                .lamports(ctx.rent_exempt_reserve + min_delegation)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
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
    .test_missing_signers(false)
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
    .test_missing_signers(false)
    .execute();
}

fn new_stake(
    stake: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteStateV4,
    activation_epoch: Epoch,
) -> Stake {
    Stake {
        delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
        credits_observed: vote_state.credits(),
    }
}

#[test]
fn test_deactivate_delinquent() {
    let mut ctx = StakeTestContext::with_delegation();

    let reference_vote_address = Pubkey::new_unique();
    let vote_address = Pubkey::new_unique();
    let stake_address = Pubkey::new_unique();

    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of());
    let stake_lamports = rent_exempt_reserve + 1;

    let initial_stake_state = StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve,
            ..Meta::default()
        },
        new_stake(1, &vote_address, &VoteStateV4::default(), 1),
        StakeFlags::empty(),
    );

    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &initial_stake_state,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let vote_rent_exempt = Rent::default().minimum_balance(VoteStateV4::size_of());
    let mut vote_account = AccountSharedData::new_data_with_space(
        vote_rent_exempt,
        &VoteStateVersions::new_v4(VoteStateV4::default()),
        VoteStateV4::size_of(),
        &solana_sdk_ids::vote::id(),
    )
    .unwrap();

    let mut reference_vote_account = AccountSharedData::new_data_with_space(
        vote_rent_exempt,
        &VoteStateVersions::new_v4(VoteStateV4::default()),
        VoteStateV4::size_of(),
        &solana_sdk_ids::vote::id(),
    )
    .unwrap();

    let current_epoch = 20;
    ctx.mollusk
        .warp_to_slot(current_epoch * ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch);

    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&stake_address, &stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&[Check::err(StakeError::InsufficientReferenceVotes.into())])
    .test_missing_signers(false)
    .execute();

    let mut reference_vote_state = VoteStateV4::default();
    for epoch in 0..MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION / 2 {
        increment_credits(&mut reference_vote_state, epoch as Epoch, 1);
    }
    reference_vote_account
        .serialize_data(&VoteStateVersions::new_v4(reference_vote_state))
        .unwrap();

    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&stake_address, &stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&[Check::err(StakeError::InsufficientReferenceVotes.into())])
    .test_missing_signers(false)
    .execute();

    let mut reference_vote_state = VoteStateV4::default();
    for epoch in 0..=current_epoch {
        increment_credits(&mut reference_vote_state, epoch, 1);
    }
    assert_eq!(
        reference_vote_state.epoch_credits[current_epoch as usize - 2].0,
        current_epoch - 2
    );
    reference_vote_state
        .epoch_credits
        .remove(current_epoch as usize - 2);
    assert_eq!(
        reference_vote_state.epoch_credits[current_epoch as usize - 2].0,
        current_epoch - 1
    );
    reference_vote_account
        .serialize_data(&VoteStateVersions::new_v4(reference_vote_state))
        .unwrap();

    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&stake_address, &stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&[Check::err(StakeError::InsufficientReferenceVotes.into())])
    .test_missing_signers(false)
    .execute();

    let mut reference_vote_state = VoteStateV4::default();
    for epoch in 0..=current_epoch {
        increment_credits(&mut reference_vote_state, epoch, 1);
    }
    reference_vote_account
        .serialize_data(&VoteStateVersions::new_v4(reference_vote_state))
        .unwrap();

    let result = ctx
        .process_with(DeactivateDelinquentConfig {
            stake: (&stake_address, &stake_account),
            vote: (&vote_address, &vote_account),
            reference_vote: (&reference_vote_address, &reference_vote_account),
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake_address)
                .lamports(stake_lamports)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();

    let post_stake_account = &result.resulting_accounts[0].1;
    let (_, stake_data, _) = parse_stake_account(&post_stake_account.clone().into());
    assert_eq!(
        stake_data.unwrap().delegation.deactivation_epoch,
        current_epoch
    );

    let mut vote_state = VoteStateV4::default();
    for epoch in 0..MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION / 2 {
        increment_credits(&mut vote_state, epoch as Epoch, 1);
    }
    vote_account
        .serialize_data(&VoteStateVersions::new_v4(vote_state))
        .unwrap();

    let result = ctx
        .process_with(DeactivateDelinquentConfig {
            stake: (&stake_address, &stake_account),
            vote: (&vote_address, &vote_account),
            reference_vote: (&reference_vote_address, &reference_vote_account),
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake_address)
                .lamports(stake_lamports)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();

    let post_stake_account = &result.resulting_accounts[0].1;
    let (_, stake_data, _) = parse_stake_account(&post_stake_account.clone().into());
    assert_eq!(
        stake_data.unwrap().delegation.deactivation_epoch,
        current_epoch
    );

    let unrelated_vote_address = Pubkey::new_unique();
    let unrelated_stake_address = Pubkey::new_unique();
    let mut unrelated_stake_account = stake_account.clone();
    assert_ne!(unrelated_vote_address, vote_address);
    unrelated_stake_account
        .serialize_data(&StakeStateV2::Stake(
            Meta::default(),
            new_stake(1, &unrelated_vote_address, &VoteStateV4::default(), 1),
            StakeFlags::empty(),
        ))
        .unwrap();

    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&unrelated_stake_address, &unrelated_stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&[Check::err(StakeError::VoteAddressMismatch.into())])
    .test_missing_signers(false)
    .execute();

    let mut vote_state = VoteStateV4::default();
    increment_credits(
        &mut vote_state,
        current_epoch - MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch,
        1,
    );
    vote_account
        .serialize_data(&VoteStateVersions::new_v4(vote_state))
        .unwrap();
    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&stake_address, &stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&[
        Check::success(),
        Check::all_rent_exempt(),
        Check::account(&stake_address)
            .lamports(stake_lamports)
            .owner(&id())
            .space(StakeStateV2::size_of())
            .build(),
    ])
    .test_missing_signers(true)
    .execute();

    let mut vote_state = VoteStateV4::default();
    increment_credits(
        &mut vote_state,
        current_epoch - (MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION - 1) as Epoch,
        1,
    );
    vote_account
        .serialize_data(&VoteStateVersions::new_v4(vote_state))
        .unwrap();
    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&stake_address, &stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&[Check::err(
        StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into(),
    )])
    .test_missing_signers(false)
    .execute();
}

#[test]
fn test_deactivate_delinquent_incorrect_vote_owner() {
    let mut ctx = StakeTestContext::with_delegation();

    let reference_vote_address = Pubkey::new_unique();
    let vote_address = Pubkey::new_unique();
    let stake_address = Pubkey::new_unique();

    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of());
    let stake_lamports = rent_exempt_reserve + 1;

    let initial_stake_state = StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve,
            ..Meta::default()
        },
        new_stake(1, &vote_address, &VoteStateV4::default(), 1),
        StakeFlags::empty(),
    );

    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &initial_stake_state,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let wrong_owner = Pubkey::new_unique();
    let vote_state = VoteStateVersions::new_v4(VoteStateV4::default());
    let vote_account = AccountSharedData::new_data_with_space(
        Rent::default().minimum_balance(VoteStateV4::size_of()),
        &vote_state,
        VoteStateV4::size_of(),
        &wrong_owner,
    )
    .unwrap();

    let mut reference_vote_state = VoteStateV4::default();
    let current_epoch = 20;
    for epoch in 0..=current_epoch {
        increment_credits(&mut reference_vote_state, epoch, 1);
    }
    let reference_vote_account = AccountSharedData::new_data_with_space(
        Rent::default().minimum_balance(VoteStateV4::size_of()),
        &VoteStateVersions::new_v4(reference_vote_state),
        VoteStateV4::size_of(),
        &solana_sdk_ids::vote::id(),
    )
    .unwrap();

    ctx.mollusk
        .warp_to_slot(current_epoch * ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch);

    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&stake_address, &stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&[Check::err(ProgramError::IncorrectProgramId)])
    .test_missing_signers(false)
    .execute();
}

enum VoteStateVersion {
    V0_23_5,
    V1_14_11,
    V3,
    V4,
}

impl VoteStateVersion {
    fn default_vote_state(&self) -> VoteStateVersions {
        match self {
            Self::V0_23_5 => VoteStateVersions::V0_23_5(Box::default()),
            Self::V1_14_11 => VoteStateVersions::V1_14_11(Box::default()),
            Self::V3 => VoteStateVersions::V3(Box::default()),
            Self::V4 => VoteStateVersions::V4(Box::default()),
        }
    }
}

#[test_case(VoteStateVersion::V0_23_5, Err(ProgramError::InvalidAccountData); "v0_23_5")]
#[test_case(VoteStateVersion::V1_14_11, Ok(()); "v1_14_11")]
#[test_case(VoteStateVersion::V3, Ok(()); "v3")]
#[test_case(VoteStateVersion::V4, Ok(()); "v4")]
fn test_deactivate_delinquent_deserialize_vote_state(
    vote_state_version: VoteStateVersion,
    expected_result: Result<(), ProgramError>,
) {
    let mut ctx = StakeTestContext::with_delegation();

    let reference_vote_address = Pubkey::new_unique();
    let vote_address = Pubkey::new_unique();
    let stake_address = Pubkey::new_unique();

    let rent_exempt_reserve = Rent::default().minimum_balance(StakeStateV2::size_of());
    let stake_lamports = rent_exempt_reserve + 1;

    let initial_stake_state = StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve,
            ..Meta::default()
        },
        new_stake(1, &vote_address, &VoteStateV4::default(), 1),
        StakeFlags::empty(),
    );

    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &initial_stake_state,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let vote_state = vote_state_version.default_vote_state();
    let vote_account = AccountSharedData::new_data_with_space(
        Rent::default().minimum_balance(VoteStateV4::size_of()),
        &vote_state,
        VoteStateV4::size_of(),
        &solana_sdk_ids::vote::id(),
    )
    .unwrap();

    let mut reference_vote_state = VoteStateV4::default();
    let current_epoch = 20;
    for epoch in 0..=current_epoch {
        increment_credits(&mut reference_vote_state, epoch, 1);
    }
    let reference_vote_account = AccountSharedData::new_data_with_space(
        Rent::default().minimum_balance(VoteStateV4::size_of()),
        &VoteStateVersions::new_v4(reference_vote_state),
        VoteStateV4::size_of(),
        &solana_sdk_ids::vote::id(),
    )
    .unwrap();

    ctx.mollusk
        .warp_to_slot(current_epoch * ctx.mollusk.sysvars.epoch_schedule.slots_per_epoch);

    let stake_program_id = id();
    let (checks, test_missing_signers) = match expected_result {
        Ok(()) => (
            vec![
                Check::success(),
                Check::all_rent_exempt(),
                Check::account(&stake_address)
                    .lamports(stake_lamports)
                    .owner(&stake_program_id)
                    .space(StakeStateV2::size_of())
                    .build(),
            ],
            true,
        ),
        Err(e) => (vec![Check::err(e)], false),
    };

    ctx.process_with(DeactivateDelinquentConfig {
        stake: (&stake_address, &stake_account),
        vote: (&vote_address, &vote_account),
        reference_vote: (&reference_vote_address, &reference_vote_account),
    })
    .checks(&checks)
    .test_missing_signers(test_missing_signers)
    .execute();
}
