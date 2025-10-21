#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    crate::helpers::{MoveLamportsConfig, MoveLamportsFullConfig},
    helpers::{
        get_effective_stake, parse_stake_account, true_up_transient_stake_epoch, StakeLifecycle,
        StakeTestContext,
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
fn test_move_lamports(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    different_votes: bool,
    has_lockup: bool,
) {
    let mut ctx = StakeTestContext::new();

    // Put minimum in both accounts if they're active
    let source_staked_amount = if move_source_type == StakeLifecycle::Active {
        ctx.minimum_delegation
    } else {
        0
    };

    let dest_staked_amount = if move_dest_type == StakeLifecycle::Active {
        ctx.minimum_delegation
    } else {
        0
    };

    // Test with and without lockup
    let lockup = if has_lockup {
        ctx.create_future_lockup(100)
    } else {
        Lockup::default()
    };

    // We put an extra minimum in every account, unstaked, to test moving them
    let source_excess = ctx.minimum_delegation;
    let dest_excess = ctx.minimum_delegation;

    // Dest vote account (possibly different)
    let (dest_vote_account, dest_vote_account_data) = if different_votes {
        ctx.create_second_vote_account()
    } else {
        (ctx.vote_account, ctx.vote_account_data.clone())
    };

    // Create source and dest stakes
    let (move_source, mut move_source_account) =
        ctx.create_stake_account_with_lockup(move_source_type, ctx.minimum_delegation, &lockup);

    let (move_dest, mut move_dest_account) = if different_votes {
        // Create with different vote account
        let dest_pubkey = solana_pubkey::Pubkey::new_unique();
        let dest_account = move_dest_type.create_stake_account_fully_specified(
            &mut ctx.mollusk,
            &mut ctx.tracker,
            &dest_pubkey,
            &dest_vote_account,
            ctx.minimum_delegation,
            &ctx.staker,
            &ctx.withdrawer,
            &lockup,
        );
        (dest_pubkey, dest_account)
    } else {
        ctx.create_stake_account_with_lockup(move_dest_type, ctx.minimum_delegation, &lockup)
    };

    // True up source epoch if transient (like original test)
    // This ensures both stakes are in the current epoch context
    true_up_transient_stake_epoch(
        &mut ctx.mollusk,
        &mut ctx.tracker,
        &move_source,
        &mut move_source_account,
        move_source_type,
    );

    // Add excess lamports if Active (like original test)
    if move_source_type == StakeLifecycle::Active {
        move_source_account
            .checked_add_lamports(source_excess)
            .unwrap();
    }
    if move_dest_type == StakeLifecycle::Active {
        move_dest_account.checked_add_lamports(dest_excess).unwrap();
    }

    // Clear out state failures (activating/deactivating not allowed)
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
        || move_dest_type == StakeLifecycle::Deactivating
    {
        let result = ctx
            .process_with(MoveLamportsFullConfig {
                source: (&move_source, &move_source_account),
                destination: (&move_dest, &move_dest_account),
                override_signer: Some(&ctx.staker),
                amount: source_excess,
                source_vote: (&ctx.vote_account, &ctx.vote_account_data),
                dest_vote: if different_votes {
                    Some((&dest_vote_account, &dest_vote_account_data))
                } else {
                    None
                },
            })
            .checks(&[])
            .execute();
        assert!(result.program_result.is_err());
        return;
    }

    // Overshoot and fail for underfunded source
    ctx.process_with(MoveLamportsFullConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest, &move_dest_account),
        override_signer: Some(&ctx.staker),
        amount: source_excess + 1,
        source_vote: (&ctx.vote_account, &ctx.vote_account_data),
        dest_vote: if different_votes {
            Some((&dest_vote_account, &dest_vote_account_data))
        } else {
            None
        },
    })
    .checks(&[Check::err(ProgramError::InvalidArgument)])
    .execute();

    let before_source_lamports = parse_stake_account(&move_source_account).2;
    let before_dest_lamports = parse_stake_account(&move_dest_account).2;

    // Now properly move the full excess
    let result = ctx
        .process_with(MoveLamportsFullConfig {
            source: (&move_source, &move_source_account),
            destination: (&move_dest, &move_dest_account),
            override_signer: Some(&ctx.staker),
            amount: source_excess,
            source_vote: (&ctx.vote_account, &ctx.vote_account_data),
            dest_vote: if different_votes {
                Some((&dest_vote_account, &dest_vote_account_data))
            } else {
                None
            },
        })
        .checks(&[Check::success()])
        .test_missing_signers(true)
        .execute();

    move_source_account = result.resulting_accounts[0].1.clone().into();
    move_dest_account = result.resulting_accounts[1].1.clone().into();

    let after_source_lamports = parse_stake_account(&move_source_account).2;
    let source_effective_stake = get_effective_stake(&ctx.mollusk, &move_source_account);

    // Source activation didn't change
    assert_eq!(source_effective_stake, source_staked_amount);

    // Source lamports are right
    assert_eq!(
        after_source_lamports,
        before_source_lamports - ctx.minimum_delegation
    );
    assert_eq!(
        after_source_lamports,
        source_effective_stake + ctx.rent_exempt_reserve
    );

    let after_dest_lamports = parse_stake_account(&move_dest_account).2;
    let dest_effective_stake = get_effective_stake(&ctx.mollusk, &move_dest_account);

    // Dest activation didn't change
    assert_eq!(dest_effective_stake, dest_staked_amount);

    // Dest lamports are right
    assert_eq!(
        after_dest_lamports,
        before_dest_lamports + ctx.minimum_delegation
    );
    assert_eq!(
        after_dest_lamports,
        dest_effective_stake + ctx.rent_exempt_reserve + source_excess + dest_excess
    );
}

#[test_matrix(
    [(StakeLifecycle::Active, StakeLifecycle::Uninitialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Initialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Uninitialized)]
)]
fn test_move_lamports_uninitialized_fail(move_types: (StakeLifecycle, StakeLifecycle)) {
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

    ctx.process_with(MoveLamportsFullConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest, &move_dest_account),
        override_signer: Some(&source_signer),
        amount: ctx.minimum_delegation,
        source_vote: (&ctx.vote_account, &ctx.vote_account_data),
        dest_vote: None,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .execute();
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active, StakeLifecycle::Deactive]
)]
fn test_move_lamports_general_fail(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
) {
    let mut ctx = StakeTestContext::new();
    let source_staked_amount = ctx.minimum_delegation * 2;
    let in_force_lockup = ctx.create_in_force_lockup();

    // Create source
    let (move_source, mut move_source_account) =
        ctx.create_stake_account(move_source_type, source_staked_amount);
    move_source_account
        .checked_add_lamports(ctx.minimum_delegation)
        .unwrap();

    // Self-move fails
    ctx.process_with(MoveLamportsConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_source, &move_source_account),
        override_signer: None,
        amount: ctx.minimum_delegation,
    })
    .checks(&[Check::err(ProgramError::InvalidInstructionData)])
    .execute();

    // Zero move fails
    let (move_dest, mut move_dest_account) =
        ctx.create_stake_account(move_dest_type, ctx.minimum_delegation);

    // True up dest epoch if transient
    true_up_transient_stake_epoch(
        &mut ctx.mollusk,
        &mut ctx.tracker,
        &move_dest,
        &mut move_dest_account,
        move_dest_type,
    );

    ctx.process_with(MoveLamportsConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest, &move_dest_account),
        override_signer: None,
        amount: 0,
    })
    .checks(&[Check::err(ProgramError::InvalidArgument)])
    .execute();

    // Sign with withdrawer fails
    ctx.process_with(MoveLamportsFullConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest, &move_dest_account),
        override_signer: Some(&ctx.withdrawer),
        amount: ctx.minimum_delegation,
        source_vote: (&ctx.vote_account, &ctx.vote_account_data),
        dest_vote: None,
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

    ctx.process_with(MoveLamportsConfig {
        source: (&move_locked_source, &move_locked_source_account),
        destination: (&move_dest2, &move_dest2_account),
        override_signer: None,
        amount: ctx.minimum_delegation,
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

    ctx.process_with(MoveLamportsConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest3, &move_dest3_account),
        override_signer: None,
        amount: ctx.minimum_delegation,
    })
    .checks(&[Check::err(StakeError::MergeMismatch.into())])
    .execute();

    ctx.process_with(MoveLamportsFullConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest3, &move_dest3_account),
        override_signer: Some(&throwaway_staker),
        amount: ctx.minimum_delegation,
        source_vote: (&ctx.vote_account, &ctx.vote_account_data),
        dest_vote: None,
    })
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
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

    ctx.process_with(MoveLamportsConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest4, &move_dest4_account),
        override_signer: None,
        amount: ctx.minimum_delegation,
    })
    .checks(&[Check::err(StakeError::MergeMismatch.into())])
    .execute();

    ctx.process_with(MoveLamportsFullConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest4, &move_dest4_account),
        override_signer: Some(&throwaway_withdrawer),
        amount: ctx.minimum_delegation,
        source_vote: (&ctx.vote_account, &ctx.vote_account_data),
        dest_vote: None,
    })
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
    .execute();

    // Dest lockup fails
    let (move_dest5, move_dest5_account) = ctx.create_stake_account_with_lockup(
        move_dest_type,
        ctx.minimum_delegation,
        &in_force_lockup,
    );

    ctx.process_with(MoveLamportsConfig {
        source: (&move_source, &move_source_account),
        destination: (&move_dest5, &move_dest5_account),
        override_signer: None,
        amount: ctx.minimum_delegation,
    })
    .checks(&[Check::err(StakeError::MergeMismatch.into())])
    .execute();
}
