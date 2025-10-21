#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{add_sysvars, create_vote_account, StakeLifecycle},
    mollusk_svm::{result::Check, Mollusk},
    solana_account::ReadableAccount,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        instruction as ixn,
        state::{Lockup, StakeStateV2},
    },
    solana_stake_program::{get_minimum_delegation, id},
    test_case::test_matrix,
};

fn mollusk_bpf() -> Mollusk {
    Mollusk::new(&id(), "solana_stake_program")
}

#[test_matrix(
    [StakeLifecycle::Uninitialized, StakeLifecycle::Initialized, StakeLifecycle::Activating,
     StakeLifecycle::Active, StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Uninitialized, StakeLifecycle::Initialized, StakeLifecycle::Activating,
     StakeLifecycle::Active, StakeLifecycle::Deactivating, StakeLifecycle::Deactive]
)]
fn test_merge(merge_source_type: StakeLifecycle, merge_dest_type: StakeLifecycle) {
    let mut mollusk = mollusk_bpf();

    let rent_exempt_reserve = helpers::STAKE_RENT_EXEMPTION;
    let minimum_delegation = get_minimum_delegation();
    let staked_amount = minimum_delegation;

    let vote_account = Pubkey::new_unique();
    let vote_account_data = create_vote_account();

    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

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

    // Create source first
    let merge_source = Pubkey::new_unique();
    let mut merge_source_account = merge_source_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        staked_amount,
        &staker,
        &withdrawer,
        &Lockup::default(),
    );

    // Retrieve source data and potentially modify authorities
    let mut source_stake_state: StakeStateV2 =
        bincode::deserialize(merge_source_account.data()).unwrap();

    // Create dest
    let merge_dest = Pubkey::new_unique();
    let merge_dest_account = merge_dest_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        staked_amount,
        &staker,
        &withdrawer,
        &Lockup::default(),
    );

    // Update source authorities to match dest and sync epochs if needed
    let clock = mollusk.sysvars.clock.clone();
    match &mut source_stake_state {
        StakeStateV2::Initialized(ref mut meta) => {
            meta.authorized.staker = staker;
            meta.authorized.withdrawer = withdrawer;
        }
        StakeStateV2::Stake(ref mut meta, ref mut stake, _) => {
            meta.authorized.staker = staker;
            meta.authorized.withdrawer = withdrawer;

            match merge_source_type {
                StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
                StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
                _ => (),
            }
        }
        _ => (),
    }

    // Store updated source
    merge_source_account.set_data(bincode::serialize(&source_stake_state).unwrap());

    // Attempt to merge
    let instructions = ixn::merge(&merge_dest, &merge_source, &staker);
    let instruction = &instructions[0];

    let accounts = vec![
        (merge_dest, merge_dest_account.clone()),
        (merge_source, merge_source_account),
        (vote_account, vote_account_data),
    ];

    if is_merge_allowed_by_type {
        helpers::process_instruction_after_testing_missing_signers(
            &mollusk,
            instruction,
            &accounts,
            &[
                Check::success(),
                Check::account(&merge_dest)
                    .lamports(staked_amount * 2 + rent_exempt_reserve * 2)
                    .owner(&id())
                    .space(StakeStateV2::size_of())
                    .rent_exempt()
                    .build(),
            ],
        );
    } else {
        // Various errors can occur for invalid merges, we just check it fails
        let accounts_with_sysvars = add_sysvars(&mollusk, instruction, accounts);
        let result = mollusk.process_instruction(instruction, &accounts_with_sysvars);
        assert!(result.program_result.is_err());
    }
}
