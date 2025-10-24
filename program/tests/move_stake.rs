#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        get_effective_stake, parse_stake_account, true_up_transient_stake_epoch, MoveStakeConfig,
        MoveStakeWithVoteConfig, StakeLifecycle, StakeTestContext,
    },
    mollusk_svm::result::Check,
    solana_account::WritableAccount,
    solana_program_error::ProgramError,
    solana_stake_interface::{error::StakeError, state::Lockup},
    test_case::test_matrix,
};

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [false, true],
    [false, true]
)]
fn test_move_stake(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    full_move: bool,
    has_lockup: bool,
) {
    let mut ctx = StakeTestContext::new();

    // Source has 2x minimum so we can easily test partial moves
    let source_staked_amount = ctx.minimum_delegation * 2;

    // This is the amount of *effective/activated* lamports for test assertions (not delegation amount)
    // All dests are created with minimum_delegation, but only Active dests have it fully activated
    let dest_staked_amount = if move_dest_type == StakeLifecycle::Active {
        ctx.minimum_delegation
    } else {
        0 // Non-Active destinations have 0 effective stake (Activating/Deactivating are transient)
    };

    // Test with and without lockup
    let lockup = if has_lockup {
        ctx.create_future_lockup(100)
    } else {
        Lockup::default()
    };

    // Extra lamports in each account to test they don't activate
    let source_excess = ctx.minimum_delegation;
    let dest_excess = ctx.minimum_delegation;

    // Create source and dest stakes
    let (move_source, mut move_source_account) =
        ctx.create_stake_account_with_lockup(move_source_type, source_staked_amount, &lockup);
    let (move_dest, mut move_dest_account) =
        ctx.create_stake_account_with_lockup(move_dest_type, ctx.minimum_delegation, &lockup);

    true_up_transient_stake_epoch(
        &mut ctx.mollusk,
        &mut ctx.tracker,
        &move_source,
        &mut move_source_account,
        move_source_type,
    );

    true_up_transient_stake_epoch(
        &mut ctx.mollusk,
        &mut ctx.tracker,
        &move_dest,
        &mut move_dest_account,
        move_dest_type,
    );

    // Add excess lamports
    move_source_account
        .checked_add_lamports(source_excess)
        .unwrap();
    // Active accounts get additional excess on top of their staked amount
    // Inactive accounts already have minimum_delegation as excess from creation
    if move_dest_type == StakeLifecycle::Active {
        move_dest_account.checked_add_lamports(dest_excess).unwrap();
    }

    // Check if this state combination is valid for MoveStake
    match (move_source_type, move_dest_type) {
        (StakeLifecycle::Active, StakeLifecycle::Initialized)
        | (StakeLifecycle::Active, StakeLifecycle::Active)
        | (StakeLifecycle::Active, StakeLifecycle::Deactive) => {
            // Valid - continue with tests
        }
        _ => {
            // Invalid state combination
            let result = ctx
                .process_with(MoveStakeConfig {
                    source: (&move_source, &move_source_account),
                    destination: (&move_dest, &move_dest_account),
                    amount: if full_move {
                        source_staked_amount
                    } else {
                        ctx.minimum_delegation
                    },
                    override_signer: None,
                })
                .checks(&[])
                .execute();
            assert!(result.program_result.is_err());
            return;
        }
    }

    // The below checks need minimum_delegation > 1
    if ctx.minimum_delegation > 1 {
        // Undershoot destination for inactive accounts
        if move_dest_type != StakeLifecycle::Active {
            ctx.process_with(MoveStakeConfig {
                source: (&move_source, &move_source_account),
                destination: (&move_dest, &move_dest_account),
                amount: ctx.minimum_delegation - 1,
                override_signer: None,
            })
            .checks(&[Check::err(ProgramError::InvalidArgument)])
            .execute();
        }

        // Overshoot source (would leave source underfunded)
        ctx.process_with(MoveStakeConfig {
            source: (&move_source, &move_source_account),
            destination: (&move_dest, &move_dest_account),
            amount: ctx.minimum_delegation + 1,
            override_signer: None,
        })
        .checks(&[Check::err(ProgramError::InvalidArgument)])
        .execute();
    }

    let result = ctx
        .process_with(MoveStakeConfig {
            source: (&move_source, &move_source_account),
            destination: (&move_dest, &move_dest_account),
            amount: if full_move {
                source_staked_amount
            } else {
                ctx.minimum_delegation
            },
            override_signer: None,
        })
        .checks(&[Check::success()])
        .test_missing_signers(true)
        .execute();

    move_source_account = result.resulting_accounts[0].1.clone().into();
    move_dest_account = result.resulting_accounts[1].1.clone().into();

    if full_move {
        let (_, option_source_stake, source_lamports) = parse_stake_account(&move_source_account);

        // Source is deactivated and rent/excess stay behind
        assert!(option_source_stake.is_none());
        assert_eq!(source_lamports, source_excess + ctx.rent_exempt_reserve);

        let (_, Some(dest_stake), dest_lamports) = parse_stake_account(&move_dest_account) else {
            panic!("dest should be active")
        };
        let dest_effective_stake = get_effective_stake(&ctx.mollusk, &move_dest_account);

        // Dest captured the entire source delegation, kept its rent/excess, didn't activate its excess
        assert_eq!(
            dest_stake.delegation.stake,
            source_staked_amount + dest_staked_amount
        );
        assert_eq!(dest_effective_stake, dest_stake.delegation.stake);
        assert_eq!(
            dest_lamports,
            dest_effective_stake + dest_excess + ctx.rent_exempt_reserve
        );
    } else {
        let (_, Some(source_stake), source_lamports) = parse_stake_account(&move_source_account)
        else {
            panic!("source should be active")
        };
        let source_effective_stake = get_effective_stake(&ctx.mollusk, &move_source_account);

        // Half of source delegation moved over, excess stayed behind
        assert_eq!(source_stake.delegation.stake, source_staked_amount / 2);
        assert_eq!(source_effective_stake, source_stake.delegation.stake);
        assert_eq!(
            source_lamports,
            source_effective_stake + source_excess + ctx.rent_exempt_reserve
        );

        let (_, Some(dest_stake), dest_lamports) = parse_stake_account(&move_dest_account) else {
            panic!("dest should be active")
        };
        let dest_effective_stake = get_effective_stake(&ctx.mollusk, &move_dest_account);

        // Dest mirrors our observations
        assert_eq!(
            dest_stake.delegation.stake,
            source_staked_amount / 2 + dest_staked_amount
        );
        assert_eq!(dest_effective_stake, dest_stake.delegation.stake);
        assert_eq!(
            dest_lamports,
            dest_effective_stake + dest_excess + ctx.rent_exempt_reserve
        );
    }
}

#[test_matrix(
    [(StakeLifecycle::Active, StakeLifecycle::Uninitialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Initialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Uninitialized)]
)]
fn test_move_stake_uninitialized_fail(move_types: (StakeLifecycle, StakeLifecycle)) {
    let mut ctx = StakeTestContext::new();
    let source_staked_amount = ctx.minimum_delegation * 2;
    let (move_source_type, move_dest_type) = move_types;

    let (move_source, move_source_account) =
        ctx.create_stake_account(move_source_type, source_staked_amount);
    let (move_dest, move_dest_account) = ctx.create_stake_account(move_dest_type, 0);

    let source_signer = if move_source_type == StakeLifecycle::Uninitialized {
        move_source
    } else {
        ctx.staker
    };

    ctx.process_with(MoveStakeConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest, &move_dest_account),
        override_signer: Some(&source_signer),
        amount: ctx.minimum_delegation,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .execute();
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active, StakeLifecycle::Deactive]
)]
fn test_move_stake_general_fail(move_source_type: StakeLifecycle, move_dest_type: StakeLifecycle) {
    let mut ctx = StakeTestContext::new();
    let source_staked_amount = ctx.minimum_delegation * 2;

    // Only test valid MoveStake combinations
    if move_source_type != StakeLifecycle::Active || move_dest_type == StakeLifecycle::Activating {
        return;
    }

    let in_force_lockup = ctx.create_in_force_lockup();

    // Create source
    let (move_source, mut move_source_account) =
        ctx.create_stake_account(move_source_type, source_staked_amount);
    move_source_account
        .checked_add_lamports(ctx.minimum_delegation)
        .unwrap();

    // Self-move fails
    ctx.process_with(MoveStakeConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_source, &move_source_account),
        amount: ctx.minimum_delegation,
        override_signer: None,
    })
    .checks(&[Check::err(ProgramError::InvalidInstructionData)])
    .execute();

    // Zero move fails
    let (move_dest, move_dest_account) =
        ctx.create_stake_account(move_dest_type, ctx.minimum_delegation);

    ctx.process_with(MoveStakeConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest, &move_dest_account),
        amount: 0,
        override_signer: None,
    })
    .checks(&[Check::err(ProgramError::InvalidArgument)])
    .execute();

    // Sign with withdrawer fails
    ctx.process_with(MoveStakeConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest, &move_dest_account),
        amount: ctx.minimum_delegation,
        override_signer: Some(&ctx.withdrawer),
    })
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
    .execute();

    // Source lockup fails
    let (move_locked_source, mut move_locked_source_account) = ctx
        .create_stake_account_with_lockup(move_source_type, source_staked_amount, &in_force_lockup);
    move_locked_source_account
        .checked_add_lamports(ctx.minimum_delegation)
        .unwrap();

    let (move_dest2, move_dest2_account) =
        ctx.create_stake_account(move_dest_type, ctx.minimum_delegation);

    ctx.process_with(MoveStakeConfig {
        source: (&move_locked_source, &move_locked_source_account),
        destination: (&move_dest2, &move_dest2_account),
        amount: ctx.minimum_delegation,
        override_signer: None,
    })
    .checks(&[Check::err(StakeError::MergeMismatch.into())])
    .execute();

    // Staker mismatch fails
    let throwaway_staker = solana_pubkey::Pubkey::new_unique();
    let withdrawer = ctx.withdrawer;
    let (move_dest3, move_dest3_account) = ctx.create_stake_account_with_authorities(
        move_dest_type,
        ctx.minimum_delegation,
        &throwaway_staker,
        &withdrawer,
    );

    ctx.process_with(MoveStakeConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest3, &move_dest3_account),
        amount: ctx.minimum_delegation,
        override_signer: None,
    })
    .checks(&[Check::err(StakeError::MergeMismatch.into())])
    .execute();

    // Withdrawer mismatch fails
    let throwaway_withdrawer = solana_pubkey::Pubkey::new_unique();
    let staker = ctx.staker;
    let (move_dest4, move_dest4_account) = ctx.create_stake_account_with_authorities(
        move_dest_type,
        ctx.minimum_delegation,
        &staker,
        &throwaway_withdrawer,
    );

    ctx.process_with(MoveStakeConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest4, &move_dest4_account),
        amount: ctx.minimum_delegation,
        override_signer: None,
    })
    .checks(&[Check::err(StakeError::MergeMismatch.into())])
    .execute();

    // Dest lockup fails
    let (move_dest5, move_dest5_account) = ctx.create_stake_account_with_lockup(
        move_dest_type,
        ctx.minimum_delegation,
        &in_force_lockup,
    );

    ctx.process_with(MoveStakeConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest5, &move_dest5_account),
        amount: ctx.minimum_delegation,
        override_signer: None,
    })
    .checks(&[Check::err(StakeError::MergeMismatch.into())])
    .execute();

    // Different vote accounts for active dest
    if move_dest_type == StakeLifecycle::Active {
        let (dest_vote_account, dest_vote_account_data) = ctx.create_second_vote_account();

        let move_dest6_pubkey = solana_pubkey::Pubkey::new_unique();
        let move_dest6_account = move_dest_type.create_stake_account_fully_specified(
            &mut ctx.mollusk,
            &mut ctx.tracker,
            &move_dest6_pubkey,
            &dest_vote_account,
            ctx.minimum_delegation,
            &ctx.staker,
            &ctx.withdrawer,
            &Lockup::default(),
        );

        let (move_source2, move_source2_account) =
            ctx.create_stake_account(move_source_type, source_staked_amount);

        ctx.process_with(MoveStakeWithVoteConfig {
            source: (&move_source2, &move_source2_account),
            destination: (&move_dest6_pubkey, &move_dest6_account),
            override_signer: Some(&ctx.staker),
            amount: ctx.minimum_delegation,
            source_vote: (&ctx.vote_account, &ctx.vote_account_data),
            dest_vote: Some((&dest_vote_account, &dest_vote_account_data)),
        })
        .checks(&[Check::err(StakeError::VoteAddressMismatch.into())])
        .execute();
    }
}
