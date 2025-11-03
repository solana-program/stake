#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        context::StakeTestContext,
        instruction_builders::{
            AuthorizeCheckedConfig, AuthorizeCheckedWithSeedConfig, AuthorizeConfig,
            InstructionExecution, WithdrawConfig,
        },
        lifecycle::StakeLifecycle,
        utils::parse_stake_account,
    },
    mollusk_svm::result::Check,
    solana_account::AccountSharedData,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_sdk_ids::system_program,
    solana_stake_interface::state::{StakeAuthorize, StakeStateV2},
    solana_stake_program::id,
    test_case::test_case,
};

#[derive(Debug, Clone, Copy)]
enum AuthorizeVariant {
    Authorize,
    AuthorizeChecked,
    AuthorizeCheckedWithSeed,
}

impl AuthorizeVariant {
    /// Returns seed parameters only if this is the AuthorizeCheckedWithSeed variant
    fn seed_params<'a>(
        &self,
        seed_base: &'a Pubkey,
        seed: &'a str,
    ) -> Option<(&'a Pubkey, &'a str)> {
        match self {
            Self::AuthorizeCheckedWithSeed => Some((seed_base, seed)),
            _ => None,
        }
    }

    fn process_authorize<'a, 'b>(
        self,
        ctx: &'a StakeTestContext,
        stake: (&'b Pubkey, &'b AccountSharedData),
        authority: &'b Pubkey,
        new_authority: &'b Pubkey,
        stake_authorize: StakeAuthorize,
        seed_params: Option<(&'b Pubkey, &'b str)>,
    ) -> InstructionExecution<'a, 'b> {
        match self {
            Self::Authorize => {
                if seed_params.is_some() {
                    panic!("Authorize variant should not have seed parameters");
                }
                ctx.process_with(AuthorizeConfig {
                    stake,
                    override_authority: Some(authority),
                    new_authority,
                    stake_authorize,
                })
            }
            Self::AuthorizeChecked => {
                if seed_params.is_some() {
                    panic!("AuthorizeChecked variant should not have seed parameters");
                }
                ctx.process_with(AuthorizeCheckedConfig {
                    stake,
                    authority,
                    new_authority,
                    stake_authorize,
                    custodian: None,
                })
            }
            Self::AuthorizeCheckedWithSeed => {
                let (authority_base, authority_seed) = seed_params
                    .expect("AuthorizeCheckedWithSeed requires seed parameters (base, seed)");
                ctx.process_with(AuthorizeCheckedWithSeedConfig {
                    stake,
                    authority_base,
                    authority_seed: authority_seed.to_string(),
                    authority_owner: &system_program::id(),
                    new_authority,
                    stake_authorize,
                    custodian: None,
                })
            }
        }
    }
}

#[test_case(AuthorizeVariant::Authorize; "authorize")]
#[test_case(AuthorizeVariant::AuthorizeChecked; "authorize_checked")]
#[test_case(AuthorizeVariant::AuthorizeCheckedWithSeed; "authorize_checked_with_seed")]
fn test_authorize(variant: AuthorizeVariant) {
    let mut ctx = StakeTestContext::new();

    // Set up seed-derived authorities for AuthorizeCheckedWithSeed variant
    let (seed_base, staker_seed, withdrawer_seed, staker1, withdrawer1) =
        if matches!(variant, AuthorizeVariant::AuthorizeCheckedWithSeed) {
            let seed_base = Pubkey::new_unique();
            let staker_seed = "staker_seed";
            let withdrawer_seed = "withdrawer_seed";
            let seeded_staker =
                Pubkey::create_with_seed(&seed_base, staker_seed, &system_program::id()).unwrap();
            let seeded_withdrawer =
                Pubkey::create_with_seed(&seed_base, withdrawer_seed, &system_program::id())
                    .unwrap();
            (
                seed_base,
                staker_seed,
                withdrawer_seed,
                seeded_staker,
                seeded_withdrawer,
            )
        } else {
            (
                Pubkey::new_unique(),
                "",
                "",
                Pubkey::new_unique(),
                Pubkey::new_unique(),
            )
        };

    let staker2 = Pubkey::new_unique();
    let staker3 = Pubkey::new_unique();

    let withdrawer2 = Pubkey::new_unique();
    let withdrawer3 = Pubkey::new_unique();

    let (stake, stake_account) = ctx.stake_account(StakeLifecycle::Uninitialized).build();

    // Authorize uninitialized fails for staker
    variant
        .process_authorize(
            &ctx,
            (&stake, &stake_account),
            &stake,
            &staker1,
            StakeAuthorize::Staker,
            variant.seed_params(&stake, ""),
        )
        .checks(&[Check::err(ProgramError::InvalidAccountData)])
        .test_missing_signers(false)
        .execute();

    // Authorize uninitialized fails for withdrawer
    variant
        .process_authorize(
            &ctx,
            (&stake, &stake_account),
            &stake,
            &withdrawer1,
            StakeAuthorize::Withdrawer,
            variant.seed_params(&stake, ""),
        )
        .checks(&[Check::err(ProgramError::InvalidAccountData)])
        .test_missing_signers(false)
        .execute();

    let (stake, mut stake_account) = ctx
        .stake_account(StakeLifecycle::Initialized)
        .stake_authority(&staker1)
        .withdraw_authority(&withdrawer1)
        .build();

    let rent_exempt_reserve = ctx.rent_exempt_reserve;

    // Change staker authority
    let result = variant
        .process_authorize(
            &ctx,
            (&stake, &stake_account),
            &staker1,
            &staker2,
            StakeAuthorize::Staker,
            variant.seed_params(&seed_base, staker_seed),
        )
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(rent_exempt_reserve)
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
    let result = variant
        .process_authorize(
            &ctx,
            (&stake, &stake_account),
            &withdrawer1,
            &withdrawer2,
            StakeAuthorize::Withdrawer,
            variant.seed_params(&seed_base, withdrawer_seed),
        )
        .checks(&[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&stake)
                .lamports(rent_exempt_reserve)
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
    variant
        .process_authorize(
            &ctx,
            (&stake, &stake_account),
            &staker1,
            &Pubkey::new_unique(),
            StakeAuthorize::Staker,
            variant.seed_params(&seed_base, staker_seed),
        )
        .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
        .test_missing_signers(false)
        .execute();

    // Old withdrawer authority no longer works
    variant
        .process_authorize(
            &ctx,
            (&stake, &stake_account),
            &withdrawer1,
            &Pubkey::new_unique(),
            StakeAuthorize::Withdrawer,
            variant.seed_params(&seed_base, withdrawer_seed),
        )
        .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
        .test_missing_signers(false)
        .execute();

    // Change staker authority again with new authority (staker2 is now the authority, not seed-derived)
    // For AuthorizeCheckedWithSeed variant, use AuthorizeChecked since authority is not seed-derived
    let result = if matches!(variant, AuthorizeVariant::AuthorizeCheckedWithSeed) {
        AuthorizeVariant::AuthorizeChecked
    } else {
        variant
    }
    .process_authorize(
        &ctx,
        (&stake, &stake_account),
        &staker2,
        &staker3,
        StakeAuthorize::Staker,
        None, // staker2 is not seed-derived, so no seed params
    )
    .checks(&[
        Check::success(),
        Check::all_rent_exempt(),
        Check::account(&stake)
            .lamports(rent_exempt_reserve)
            .owner(&id())
            .space(StakeStateV2::size_of())
            .build(),
    ])
    .test_missing_signers(true)
    .execute();
    stake_account = result.resulting_accounts[0].1.clone().into();

    let (meta, _, _) = parse_stake_account(&stake_account);
    assert_eq!(meta.authorized.staker, staker3);

    // Change withdrawer authority again with new authority (withdrawer2 is now the authority, not seed-derived)
    // For AuthorizeCheckedWithSeed variant, use AuthorizeChecked since authority is not seed-derived
    let result = if matches!(variant, AuthorizeVariant::AuthorizeCheckedWithSeed) {
        AuthorizeVariant::AuthorizeChecked
    } else {
        variant
    }
    .process_authorize(
        &ctx,
        (&stake, &stake_account),
        &withdrawer2,
        &withdrawer3,
        StakeAuthorize::Withdrawer,
        None, // withdrawer2 is not seed-derived, so no seed params
    )
    .checks(&[
        Check::success(),
        Check::all_rent_exempt(),
        Check::account(&stake)
            .lamports(rent_exempt_reserve)
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
    // For AuthorizeCheckedWithSeed variant, use AuthorizeChecked since authority is not seed-derived
    if matches!(variant, AuthorizeVariant::AuthorizeCheckedWithSeed) {
        AuthorizeVariant::AuthorizeChecked
    } else {
        variant
    }
    .process_authorize(
        &ctx,
        (&stake, &stake_account),
        &staker3,
        &Pubkey::new_unique(),
        StakeAuthorize::Withdrawer,
        None, // staker3 is not seed-derived
    )
    .checks(&[Check::err(ProgramError::MissingRequiredSignature)])
    .test_missing_signers(false)
    .execute();

    // Changing staker using withdrawer is fine
    // For AuthorizeCheckedWithSeed variant, use AuthorizeChecked since authority is not seed-derived
    let result = if matches!(variant, AuthorizeVariant::AuthorizeCheckedWithSeed) {
        AuthorizeVariant::AuthorizeChecked
    } else {
        variant
    }
    .process_authorize(
        &ctx,
        (&stake, &stake_account),
        &withdrawer3,
        &staker1,
        StakeAuthorize::Staker,
        None, // withdrawer3 is not seed-derived
    )
    .checks(&[
        Check::success(),
        Check::all_rent_exempt(),
        Check::account(&stake)
            .lamports(rent_exempt_reserve)
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
        .test_missing_signers(false)
        .execute();
    }
}
