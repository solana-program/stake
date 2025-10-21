#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{add_sysvars, get_effective_stake, parse_stake_account, StakeLifecycle},
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, WritableAccount},
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::{instruction as ixn, state::StakeStateV2},
    solana_stake_program::{get_minimum_delegation, id},
    test_case::test_case,
};

fn mollusk_bpf() -> Mollusk {
    Mollusk::new(&id(), "solana_stake_program")
}

#[test_case(StakeLifecycle::Uninitialized; "uninitialized")]
#[test_case(StakeLifecycle::Initialized; "initialized")]
#[test_case(StakeLifecycle::Activating; "activating")]
#[test_case(StakeLifecycle::Active; "active")]
#[test_case(StakeLifecycle::Deactivating; "deactivating")]
#[test_case(StakeLifecycle::Deactive; "deactive")]
fn test_split(split_source_type: StakeLifecycle) {
    let mut mollusk = mollusk_bpf();

    let rent_exempt_reserve = helpers::STAKE_RENT_EXEMPTION;
    let minimum_delegation = get_minimum_delegation();
    let staked_amount = minimum_delegation * 2;

    let vote_account = Pubkey::new_unique();
    let staker = Pubkey::new_unique();
    let withdrawer = Pubkey::new_unique();

    // Create source stake account at the specified lifecycle stage
    let split_source = Pubkey::new_unique();
    let mut split_source_account = split_source_type.create_stake_account_fully_specified(
        &mut mollusk,
        &vote_account,
        staked_amount,
        &staker,
        &withdrawer,
        &solana_stake_interface::state::Lockup::default(),
    );

    // Create destination stake account matching what create_blank_stake_account does:
    // rent-exempt lamports, correct size, stake program owner, uninitialized data
    let split_dest = Pubkey::new_unique();
    let split_dest_account = AccountSharedData::new(
        rent_exempt_reserve, // Match the original test setup
        StakeStateV2::size_of(),
        &id(),
    );

    // Determine signer based on lifecycle stage
    let signer = if split_source_type == StakeLifecycle::Uninitialized {
        split_source // Self-signed for uninitialized
    } else {
        staker
    };

    // Fail: split more than available (would violate rent exemption)
    let instructions = ixn::split(&split_source, &signer, staked_amount + 1, &split_dest);
    let instruction = &instructions[2]; // The actual split instruction

    let accounts = vec![
        (split_source, split_source_account.clone()),
        (split_dest, split_dest_account.clone()),
    ];

    let accounts = add_sysvars(&mollusk, instruction, accounts);

    // For initialized/delegated accounts, the program itself checks and fails with InsufficientFunds
    // For uninitialized accounts, the program succeeds but leaves accounts below rent exemption
    if split_source_type == StakeLifecycle::Uninitialized {
        // Expect program success but rent check should fail - catch the panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            mollusk.process_and_validate_instruction(
                instruction,
                &accounts,
                &[Check::success(), Check::all_rent_exempt()],
            )
        }));
        // The rent exemption check should panic
        assert!(
            result.is_err(),
            "Expected rent exemption check to fail for uninitialized split"
        );
    } else {
        // Program fails with InsufficientFunds
        mollusk.process_and_validate_instruction(
            instruction,
            &accounts,
            &[Check::err(ProgramError::InsufficientFunds)],
        );
    }

    // Test minimum delegation enforcement for active/transitioning stakes
    if split_source_type.split_minimum_enforced() {
        // Zero split fails
        let instructions = ixn::split(&split_source, &signer, 0, &split_dest);
        let instruction = &instructions[2];

        let accounts = vec![
            (split_source, split_source_account.clone()),
            (split_dest, split_dest_account.clone()),
        ];

        let accounts = add_sysvars(&mollusk, instruction, accounts);
        mollusk.process_and_validate_instruction(
            instruction,
            &accounts,
            &[Check::err(ProgramError::InsufficientFunds)],
        );

        // Underfunded destination fails
        let instructions = ixn::split(&split_source, &signer, minimum_delegation - 1, &split_dest);
        let instruction = &instructions[2];

        let accounts = vec![
            (split_source, split_source_account.clone()),
            (split_dest, split_dest_account.clone()),
        ];

        let accounts = add_sysvars(&mollusk, instruction, accounts);
        mollusk.process_and_validate_instruction(
            instruction,
            &accounts,
            &[Check::err(ProgramError::InsufficientFunds)],
        );

        // Underfunded source fails
        let instructions = ixn::split(&split_source, &signer, minimum_delegation + 1, &split_dest);
        let instruction = &instructions[2];

        let accounts = vec![
            (split_source, split_source_account.clone()),
            (split_dest, split_dest_account.clone()),
        ];

        let accounts = add_sysvars(&mollusk, instruction, accounts);
        mollusk.process_and_validate_instruction(
            instruction,
            &accounts,
            &[Check::err(ProgramError::InsufficientFunds)],
        );
    }

    // Split to account with wrong owner fails
    let fake_split_dest = Pubkey::new_unique();
    let mut fake_split_dest_account = split_dest_account.clone();
    fake_split_dest_account.set_owner(Pubkey::new_unique());

    let instructions = ixn::split(&split_source, &signer, staked_amount / 2, &fake_split_dest);
    let instruction = &instructions[2];

    let accounts = vec![
        (split_source, split_source_account.clone()),
        (fake_split_dest, fake_split_dest_account),
    ];

    let accounts = add_sysvars(&mollusk, instruction, accounts);
    mollusk.process_and_validate_instruction(
        instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidAccountOwner)],
    );

    // Success: split half
    let instructions = ixn::split(&split_source, &signer, staked_amount / 2, &split_dest);
    let instruction = &instructions[2];

    let accounts = vec![
        (split_source, split_source_account.clone()),
        (split_dest, split_dest_account),
    ];

    let result = helpers::process_instruction_after_testing_missing_signers(
        &mollusk,
        instruction,
        &accounts,
        &[
            Check::success(),
            Check::all_rent_exempt(),
            Check::account(&split_source)
                .lamports(staked_amount / 2 + rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
            Check::account(&split_dest)
                .lamports(staked_amount / 2 + rent_exempt_reserve)
                .owner(&id())
                .space(StakeStateV2::size_of())
                .build(),
        ],
    );

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
            get_effective_stake(&mollusk, &split_source_account),
            staked_amount / 2,
        );

        assert_eq!(
            get_effective_stake(&mollusk, &split_dest_account),
            staked_amount / 2,
        );
    }
}
