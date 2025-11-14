#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        context::StakeTestContext, instruction_builders::MergeConfig, lifecycle::StakeLifecycle,
    },
    mollusk_svm::result::Check,
    solana_account::ReadableAccount,
    solana_stake_interface::state::StakeStateV2,
    solana_stake_program::id,
    test_case::test_matrix,
};

#[test_matrix(
    [StakeLifecycle::Uninitialized, StakeLifecycle::Initialized, StakeLifecycle::Activating,
     StakeLifecycle::Active, StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Uninitialized, StakeLifecycle::Initialized, StakeLifecycle::Activating,
     StakeLifecycle::Active, StakeLifecycle::Deactivating, StakeLifecycle::Deactive]
)]
fn test_merge(merge_source_type: StakeLifecycle, merge_dest_type: StakeLifecycle) {
    let mut ctx = StakeTestContext::new();

    let staked_amount = ctx.minimum_delegation;

    // Determine if merge should be allowed based on lifecycle types
    let is_merge_allowed_by_type = match (merge_source_type, merge_dest_type) {
        // Inactive and inactive
        (StakeLifecycle::Initialized, StakeLifecycle::Initialized)
        | (StakeLifecycle::Initialized, StakeLifecycle::Deactive)
        | (StakeLifecycle::Deactive, StakeLifecycle::Initialized)
        | (StakeLifecycle::Deactive, StakeLifecycle::Deactive) => true,

        // Activating into inactive is also allowed
        (StakeLifecycle::Activating, StakeLifecycle::Initialized)
        | (StakeLifecycle::Activating, StakeLifecycle::Deactive) => true,

        // Inactive into activating
        (StakeLifecycle::Initialized, StakeLifecycle::Activating)
        | (StakeLifecycle::Deactive, StakeLifecycle::Activating) => true,

        // Active and active
        (StakeLifecycle::Active, StakeLifecycle::Active) => true,

        // Activating and activating
        (StakeLifecycle::Activating, StakeLifecycle::Activating) => true,

        // Everything else fails
        _ => false,
    };

    // Create source and dest accounts
    let (merge_source, mut merge_source_account) = ctx
        .stake_account(merge_source_type)
        .staked_amount(staked_amount.unwrap())
        .build();
    let (merge_dest, merge_dest_account) = ctx
        .stake_account(merge_dest_type)
        .staked_amount(staked_amount.unwrap())
        .build();

    // Retrieve source data and sync epochs if needed
    let mut source_stake_state: StakeStateV2 =
        bincode::deserialize(merge_source_account.data()).unwrap();

    let clock = ctx.mollusk.sysvars.clock.clone();
    // Sync epochs for transient states
    if let StakeStateV2::Stake(_, ref mut stake, _) = &mut source_stake_state {
        match merge_source_type {
            StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
            StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
            _ => (),
        }
    }

    // Store updated source
    merge_source_account.set_data(bincode::serialize(&source_stake_state).unwrap());

    // Attempt to merge
    if is_merge_allowed_by_type {
        ctx.process_with(MergeConfig {
            destination: (&merge_dest, &merge_dest_account),
            source: (&merge_source, &merge_source_account),
        })
        .checks(&[
            Check::success(),
            Check::account(&merge_dest)
                .lamports(staked_amount.unwrap() * 2 + ctx.rent_exempt_reserve * 2)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .rent_exempt()
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    } else {
        // Various errors can occur for invalid merges, we just check it fails
        let result = ctx
            .process_with(MergeConfig {
                destination: (&merge_dest, &merge_dest_account),
                source: (&merge_source, &merge_source_account),
            })
            .checks(&[]) // Skip Success check
            .test_missing_signers(false)
            .execute();
        assert!(result.program_result.is_err());
    }
}
