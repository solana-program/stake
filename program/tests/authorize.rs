#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        add_sysvars, initialize_stake_account, parse_stake_account,
        process_instruction_after_testing_missing_signers, StakeTestContext,
    },
    mollusk_svm::result::Check,
    solana_account::AccountSharedData,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup, StakeAuthorize, StakeStateV2},
    },
    solana_stake_program::id,
};

#[test]
fn test_authorize() {
    let ctx = StakeTestContext::new();

    let staker1 = Pubkey::new_unique();
    let staker2 = Pubkey::new_unique();
    let staker3 = Pubkey::new_unique();

    let withdrawer1 = Pubkey::new_unique();
    let withdrawer2 = Pubkey::new_unique();
    let withdrawer3 = Pubkey::new_unique();

    let stake = Pubkey::new_unique();
    let stake_account = AccountSharedData::new_data_with_space(
        ctx.rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    // Authorize uninitialized fails for staker
    let instruction = ixn::authorize(&stake, &stake, &staker1, StakeAuthorize::Staker, None);
    let accounts = vec![(stake, stake_account.clone())];
    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);

    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );

    // Authorize uninitialized fails for withdrawer
    let instruction = ixn::authorize(
        &stake,
        &stake,
        &withdrawer1,
        StakeAuthorize::Withdrawer,
        None,
    );
    let accounts = vec![(stake, stake_account.clone())];
    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);

    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountData)],
    );

    let mut stake_account = initialize_stake_account(
        &ctx.mollusk,
        &stake,
        ctx.rent_exempt_reserve,
        &Authorized {
            staker: staker1,
            withdrawer: withdrawer1,
        },
        &Lockup::default(),
    );

    // Change staker authority
    let instruction = ixn::authorize(&stake, &staker1, &staker2, StakeAuthorize::Staker, None);
    let accounts = vec![(stake, stake_account.clone())];

    // Test that removing any signer causes failure, then verify success
    let result = process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.staker, staker2);

    // Change withdrawer authority
    let instruction = ixn::authorize(
        &stake,
        &withdrawer1,
        &withdrawer2,
        StakeAuthorize::Withdrawer,
        None,
    );
    let accounts = vec![(stake, stake_account.clone())];

    // Test that removing any signer causes failure, then verify success
    let result = process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.withdrawer, withdrawer2);

    // Old staker authority no longer works
    let instruction = ixn::authorize(
        &stake,
        &staker1,
        &Pubkey::new_unique(),
        StakeAuthorize::Staker,
        None,
    );
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Old withdrawer authority no longer works
    let instruction = ixn::authorize(
        &stake,
        &withdrawer1,
        &Pubkey::new_unique(),
        StakeAuthorize::Withdrawer,
        None,
    );
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Change staker authority again with new authority
    let instruction = ixn::authorize(&stake, &staker2, &staker3, StakeAuthorize::Staker, None);
    let accounts = vec![(stake, stake_account.clone())];

    // Test that removing any signer causes failure, then verify success
    let result = process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.staker, staker3);

    // Change withdrawer authority again with new authority
    let instruction = ixn::authorize(
        &stake,
        &withdrawer2,
        &withdrawer3,
        StakeAuthorize::Withdrawer,
        None,
    );
    let accounts = vec![(stake, stake_account.clone())];

    // Test that removing any signer causes failure, then verify success
    let result = process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.withdrawer, withdrawer3);

    // Changing withdrawer using staker fails
    let instruction = ixn::authorize(
        &stake,
        &staker3,
        &Pubkey::new_unique(),
        StakeAuthorize::Withdrawer,
        None,
    );
    let accounts = vec![(stake, stake_account.clone())];

    let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
    ctx.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );

    // Changing staker using withdrawer is fine
    let instruction = ixn::authorize(&stake, &withdrawer3, &staker1, StakeAuthorize::Staker, None);
    let accounts = vec![(stake, stake_account.clone())];

    // Test that removing any signer causes failure, then verify success
    let result = process_instruction_after_testing_missing_signers(
        &ctx.mollusk,
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(ctx.rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.staker, staker1);

    // Withdraw using staker fails - test all three stakers to ensure none can withdraw
    for staker in [staker1, staker2, staker3] {
        let recipient = Pubkey::new_unique();
        let instruction = ixn::withdraw(&stake, &staker, &recipient, ctx.rent_exempt_reserve, None);
        let accounts = vec![
            (stake, stake_account.clone()),
            (recipient, AccountSharedData::default()),
        ];

        let accounts = add_sysvars(&ctx.mollusk, &instruction, accounts);
        ctx.mollusk.process_and_validate_instruction(
            &instruction,
            &accounts,
            &[Check::err(ProgramError::MissingRequiredSignature)],
        );
    }
}
