#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{parse_stake_account, AuthorizeConfig, StakeTestContext, WithdrawConfig},
    mollusk_svm::result::Check,
    solana_account::AccountSharedData,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::state::{StakeAuthorize, StakeStateV2},
    solana_stake_program::id,
};

#[test]
fn test_authorize() {
    let mut ctx = StakeTestContext::new();

    let staker1 = Pubkey::new_unique();
    let staker2 = Pubkey::new_unique();
    let staker3 = Pubkey::new_unique();

    let withdrawer1 = Pubkey::new_unique();
    let withdrawer2 = Pubkey::new_unique();
    let withdrawer3 = Pubkey::new_unique();

    let (stake, stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Uninitialized)
        .build();

    // Authorize uninitialized fails for staker
    ctx.process_with(AuthorizeConfig {
        stake: (&stake, &stake_account),
        override_authority: Some(&stake),
        new_authority: &staker1,
        stake_authorize: StakeAuthorize::Staker,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .execute();

    // Authorize uninitialized fails for withdrawer
    ctx.process_with(AuthorizeConfig {
        stake: (&stake, &stake_account),
        override_authority: Some(&stake),
        new_authority: &withdrawer1,
        stake_authorize: StakeAuthorize::Withdrawer,
    })
    .checks(&[Check::err(ProgramError::InvalidAccountData)])
    .execute();

    let (stake, mut stake_account) = ctx
        .stake_account(helpers::StakeLifecycle::Initialized)
        .stake_authority(&staker1)
        .withdraw_authority(&withdrawer1)
        .build();

    // Change staker authority
    // Test that removing any signer causes failure, then verify success
    let result = ctx
        .process_with(AuthorizeConfig {
            stake: (&stake, &stake_account),
            override_authority: Some(&staker1),
            new_authority: &staker2,
            stake_authorize: StakeAuthorize::Staker,
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.staker, staker2);

    // Change withdrawer authority
    // Test that removing any signer causes failure, then verify success
    let result = ctx
        .process_with(AuthorizeConfig {
            stake: (&stake, &stake_account),
            override_authority: Some(&withdrawer1),
            new_authority: &withdrawer2,
            stake_authorize: StakeAuthorize::Withdrawer,
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.withdrawer, withdrawer2);

    // Old staker authority no longer works
    ctx.process_with(AuthorizeConfig {
        stake: (&stake, &stake_account),
        override_authority: Some(&staker1),
        new_authority: &Pubkey::new_unique(),
        stake_authorize: StakeAuthorize::Staker,
    })
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
    .execute();

    // Old withdrawer authority no longer works
    ctx.process_with(AuthorizeConfig {
        stake: (&stake, &stake_account),
        override_authority: Some(&withdrawer1),
        new_authority: &Pubkey::new_unique(),
        stake_authorize: StakeAuthorize::Withdrawer,
    })
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
    .execute();

    // Change staker authority again with new authority
    // Test that removing any signer causes failure, then verify success
    let result = ctx
        .process_with(AuthorizeConfig {
            stake: (&stake, &stake_account),
            override_authority: Some(&staker2),
            new_authority: &staker3,
            stake_authorize: StakeAuthorize::Staker,
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.staker, staker3);

    // Change withdrawer authority again with new authority
    // Test that removing any signer causes failure, then verify success
    let result = ctx
        .process_with(AuthorizeConfig {
            stake: (&stake, &stake_account),
            override_authority: Some(&withdrawer2),
            new_authority: &withdrawer3,
            stake_authorize: StakeAuthorize::Withdrawer,
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.withdrawer, withdrawer3);

    // Changing withdrawer using staker fails
    ctx.process_with(AuthorizeConfig {
        stake: (&stake, &stake_account),
        override_authority: Some(&staker3),
        new_authority: &Pubkey::new_unique(),
        stake_authorize: StakeAuthorize::Withdrawer,
    })
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
    .execute();

    // Changing staker using withdrawer is fine
    // Test that removing any signer causes failure, then verify success
    let result = ctx
        .process_with(AuthorizeConfig {
            stake: (&stake, &stake_account),
            override_authority: Some(&withdrawer3),
            new_authority: &staker1,
            stake_authorize: StakeAuthorize::Staker,
        })
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ])
        .test_missing_signers(true)
        .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.staker, staker1);

    // Withdraw using staker fails - test all three stakers to ensure none can withdraw
    for staker in [staker1, staker2, staker3] {
        let recipient = Pubkey::new_unique();
        ctx.process_with(WithdrawConfig {
            stake: (&stake, &stake_account),
            override_signer: Some(&staker),
            recipient: (&recipient, &AccountSharedData::default()),
            amount: ctx.rent_exempt_reserve,
        })
        .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
        .execute();
    }
}
