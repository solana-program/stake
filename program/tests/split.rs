#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        get_effective_stake, parse_stake_account, SplitConfig, StakeLifecycle, StakeTestContext,
    },
    mollusk_svm::result::Check,
    solana_account::{AccountSharedData, WritableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
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
fn test_split(split_source_type: StakeLifecycle) {
    let mut ctx = StakeTestContext::new();
    let staked_amount = ctx.minimum_delegation * 2;

    // Create source stake account at the specified lifecycle stage
    let (split_source, mut split_source_account) =
        ctx.create_stake_account(split_source_type, staked_amount);

    // Create destination stake account matching what create_blank_stake_account does:
    // rent-exempt lamports, correct size, stake program owner, uninitialized data
    let split_dest = Pubkey::new_unique();
    let split_dest_account =
        AccountSharedData::new(ctx.rent_exempt_reserve, StakeStateV2::size_of(), &id());

    // Determine signer based on lifecycle stage
    let signer = if split_source_type == StakeLifecycle::Uninitialized {
        split_source
    } else {
        ctx.staker
    };

    // Fail: split more than available (would violate rent exemption)
    // For initialized/delegated accounts, the program itself checks and fails with InsufficientFunds
    // For uninitialized accounts, the program succeeds but leaves accounts below rent exemption
    if split_source_type == StakeLifecycle::Uninitialized {
        // Expect program success but rent check should fail - catch the panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.process_with(SplitConfig {
                source: (&split_source, &split_source_account),
                destination: (&split_dest, &split_dest_account),
                signer: &signer,
                amount: staked_amount + 1,
            })
            .checks(&[Check::success(), Check::all_rent_exempt()])
            .execute()
        }));
        // The rent exemption check should panic
        assert!(
            result.is_err(),
            "Expected rent exemption check to fail for uninitialized split"
        );
    } else {
        // Program fails with InsufficientFunds
        ctx.process_with(SplitConfig {
            source: (&split_source, &split_source_account),
            destination: (&split_dest, &split_dest_account),
            signer: &signer,
            amount: staked_amount + 1,
        })
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .execute();
    }

    // Test minimum delegation enforcement for active/transitioning stakes
    if split_source_type.split_minimum_enforced() {
        // Zero split fails
        ctx.process_with(SplitConfig {
            source: (&split_source, &split_source_account),
            destination: (&split_dest, &split_dest_account),
            signer: &signer,
            amount: 0,
        })
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .execute();

        // Underfunded destination fails
        ctx.process_with(SplitConfig {
            source: (&split_source, &split_source_account),
            destination: (&split_dest, &split_dest_account),
            signer: &signer,
            amount: ctx.minimum_delegation - 1,
        })
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .execute();

        // Underfunded source fails
        ctx.process_with(SplitConfig {
            source: (&split_source, &split_source_account),
            destination: (&split_dest, &split_dest_account),
            signer: &signer,
            amount: ctx.minimum_delegation + 1,
        })
        .checks(&[Check::err(ProgramError::InsufficientFunds)])
        .execute();
    }

    // Split to account with wrong owner fails
    let fake_split_dest = Pubkey::new_unique();
    let mut fake_split_dest_account = split_dest_account.clone();
    fake_split_dest_account.set_owner(Pubkey::new_unique());

    ctx.process_with(SplitConfig {
        source: (&split_source, &split_source_account),
        destination: (&fake_split_dest, &fake_split_dest_account),
        signer: &signer,
        amount: staked_amount / 2,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountOwner)])
    .execute();

    // Success: split half
    let result = ctx
        .process_with(SplitConfig {
            source: (&split_source, &split_source_account),
            destination: (&split_dest, &split_dest_account),
            signer: &signer,
            amount: staked_amount / 2,
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&split_source)
                .lamports(staked_amount / 2 + ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
            Check::account(&split_dest)
                .lamports(staked_amount / 2 + ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();

    split_source_account = result.resulting_accounts[0].1.clone().into();
    let split_dest_account: AccountSharedData = result.resulting_accounts[1].1.clone().into();

    // Verify metadata is copied for initialized and above
    if split_source_type >= StakeLifecycle::Initialized {
        let (source_meta, source_stake, _) = parse_stake_account(&split_source_account);
        let (dest_meta, dest_stake, _) = parse_stake_account(&split_dest_account);
        assert_eq!(dest_meta, source_meta);

        // Verify delegations are set properly for activating/active/deactivating
        if split_source_type >= StakeLifecycle::Activating
            && split_source_type < StakeLifecycle::Deactive
        {
            assert_eq!(source_stake.unwrap().delegation.stake, staked_amount / 2);
            assert_eq!(dest_stake.unwrap().delegation.stake, staked_amount / 2);
        }
    }

    // Verify nothing has been deactivated for active stakes
    if split_source_type >= StakeLifecycle::Active && split_source_type < StakeLifecycle::Deactive {
        assert_eq!(
            get_effective_stake(&ctx.mollusk, &split_source_account),
            staked_amount / 2,
        );

        assert_eq!(
            get_effective_stake(&ctx.mollusk, &split_dest_account),
            staked_amount / 2,
        );
    }
}
