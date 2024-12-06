#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::arithmetic_side_effects)]

use {
    bincode::serialize,
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, ReadableAccount, WritableAccount},
    solana_sdk::{
        account::{create_account_shared_data_for_test, Account as SolanaAccount},
        account_utils::StateMut,
        address_lookup_table, bpf_loader_upgradeable,
        entrypoint::ProgramResult,
        feature_set::{
            enable_partitioned_epoch_reward, get_sysvar_syscall_enabled,
            move_stake_and_move_lamports_ixs, partitioned_epoch_rewards_superfeature,
            stake_raise_minimum_delegation_to_1_sol,
        },
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        native_token::LAMPORTS_PER_SOL,
        program_error::ProgramError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        signers::Signers,
        stake::{
            self,
            instruction::{self, LockupArgs, LockupCheckedArgs, StakeError, StakeInstruction},
            stake_flags::StakeFlags,
            state::{
                warmup_cooldown_rate, Authorized, Delegation, Lockup, Meta, Stake,
                StakeActivationStatus, StakeAuthorize, StakeStateV2,
            },
            MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION,
        },
        stake_history::{Epoch, StakeHistoryEntry},
        system_instruction, system_program,
        sysvar::{
            clock::{self, Clock},
            epoch_rewards::EpochRewards,
            epoch_schedule::EpochSchedule,
            rent::Rent,
            stake_history::{self, StakeHistory},
            SysvarId,
        },
        transaction::{Transaction, TransactionError},
        vote::{
            program as solana_vote_program,
            state::{VoteInit, VoteState, VoteStateVersions},
        },
    },
    solana_stake_program::{get_minimum_delegation, id, processor::Processor},
    std::{collections::HashMap, fs, sync::Arc},
    test_case::{test_case, test_matrix},
};

fn mollusk_native() -> Mollusk {
    let mut mollusk = Mollusk::default();
    mollusk
        .feature_set
        .deactivate(&stake_raise_minimum_delegation_to_1_sol::id());
    mollusk
}

fn mollusk_bpf() -> Mollusk {
    let mut mollusk = Mollusk::new(&id(), "solana_stake_program");
    mollusk
        .feature_set
        .deactivate(&stake_raise_minimum_delegation_to_1_sol::id());
    mollusk
}

fn process_instruction(
    mollusk: &Mollusk,
    instruction_data: &[u8],
    transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
    instruction_accounts: Vec<AccountMeta>,
    expected_result: Result<(), ProgramError>,
) -> Vec<AccountSharedData> {
    let instruction = Instruction {
        program_id: id(),
        accounts: instruction_accounts,
        data: instruction_data.to_vec(),
    };

    let check = match expected_result {
        Ok(()) => Check::success(),
        Err(e) => Check::err(e),
    };

    let result =
        mollusk.process_and_validate_instruction(&instruction, &transaction_accounts, &[check]);

    result
        .resulting_accounts
        .into_iter()
        .map(|(_, account)| account)
        .collect()
}

fn new_stake(
    stake: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    activation_epoch: Epoch,
) -> Stake {
    Stake {
        delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
        credits_observed: vote_state.credits(),
    }
}

fn from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<StakeStateV2> {
    account.state().ok()
}

fn stake_from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<Stake> {
    from(account).and_then(|state: StakeStateV2| state.stake())
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_deactivate_delinquent(mollusk: Mollusk) {
    let reference_vote_address = Pubkey::new_unique();
    let vote_address = Pubkey::new_unique();
    let stake_address = Pubkey::new_unique();

    let initial_stake_state = StakeStateV2::Stake(
        Meta::default(),
        new_stake(
            1, /* stake */
            &vote_address,
            &VoteState::default(),
            1, /* activation_epoch */
        ),
        StakeFlags::empty(),
    );

    let stake_account = AccountSharedData::new_data_with_space(
        1, /* lamports */
        &initial_stake_state,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let mut vote_account = AccountSharedData::new_data_with_space(
        1, /* lamports */
        &VoteStateVersions::new_current(VoteState::default()),
        VoteState::size_of(),
        &solana_vote_program::id(),
    )
    .unwrap();

    let mut reference_vote_account = AccountSharedData::new_data_with_space(
        1, /* lamports */
        &VoteStateVersions::new_current(VoteState::default()),
        VoteState::size_of(),
        &solana_vote_program::id(),
    )
    .unwrap();

    let current_epoch = 20;

    let process_instruction_deactivate_delinquent =
        |stake_address: &Pubkey,
         stake_account: &AccountSharedData,
         vote_account: &AccountSharedData,
         reference_vote_account: &AccountSharedData,
         expected_result| {
            process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::DeactivateDelinquent).unwrap(),
                vec![
                    (*stake_address, stake_account.clone()),
                    (vote_address, vote_account.clone()),
                    (reference_vote_address, reference_vote_account.clone()),
                    (
                        clock::id(),
                        create_account_shared_data_for_test(&Clock {
                            epoch: current_epoch,
                            ..Clock::default()
                        }),
                    ),
                    (
                        stake_history::id(),
                        create_account_shared_data_for_test(&StakeHistory::default()),
                    ),
                ],
                vec![
                    AccountMeta {
                        pubkey: *stake_address,
                        is_signer: false,
                        is_writable: true,
                    },
                    AccountMeta {
                        pubkey: vote_address,
                        is_signer: false,
                        is_writable: false,
                    },
                    AccountMeta {
                        pubkey: reference_vote_address,
                        is_signer: false,
                        is_writable: false,
                    },
                ],
                expected_result,
            )
        };

    // `reference_vote_account` has not voted. Instruction will fail
    process_instruction_deactivate_delinquent(
        &stake_address,
        &stake_account,
        &vote_account,
        &reference_vote_account,
        Err(StakeError::InsufficientReferenceVotes.into()),
    );

    // `reference_vote_account` has not consistently voted for at least
    // `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`.
    // Instruction will fail
    let mut reference_vote_state = VoteState::default();
    for epoch in 0..MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION / 2 {
        reference_vote_state.increment_credits(epoch as Epoch, 1);
    }
    reference_vote_account
        .serialize_data(&VoteStateVersions::new_current(reference_vote_state))
        .unwrap();

    process_instruction_deactivate_delinquent(
        &stake_address,
        &stake_account,
        &vote_account,
        &reference_vote_account,
        Err(StakeError::InsufficientReferenceVotes.into()),
    );

    // `reference_vote_account` has not consistently voted for the last
    // `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`.
    // Instruction will fail
    let mut reference_vote_state = VoteState::default();
    for epoch in 0..=current_epoch {
        reference_vote_state.increment_credits(epoch, 1);
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
        .serialize_data(&VoteStateVersions::new_current(reference_vote_state))
        .unwrap();

    process_instruction_deactivate_delinquent(
        &stake_address,
        &stake_account,
        &vote_account,
        &reference_vote_account,
        Err(StakeError::InsufficientReferenceVotes.into()),
    );

    // `reference_vote_account` has consistently voted and `vote_account` has never voted.
    // Instruction will succeed
    let mut reference_vote_state = VoteState::default();
    for epoch in 0..=current_epoch {
        reference_vote_state.increment_credits(epoch, 1);
    }
    reference_vote_account
        .serialize_data(&VoteStateVersions::new_current(reference_vote_state))
        .unwrap();

    let post_stake_account = &process_instruction_deactivate_delinquent(
        &stake_address,
        &stake_account,
        &vote_account,
        &reference_vote_account,
        Ok(()),
    )[0];

    assert_eq!(
        stake_from(post_stake_account)
            .unwrap()
            .delegation
            .deactivation_epoch,
        current_epoch
    );

    // `reference_vote_account` has consistently voted and `vote_account` has not voted for the
    // last `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`.
    // Instruction will succeed

    let mut vote_state = VoteState::default();
    for epoch in 0..MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION / 2 {
        vote_state.increment_credits(epoch as Epoch, 1);
    }
    vote_account
        .serialize_data(&VoteStateVersions::new_current(vote_state))
        .unwrap();

    let post_stake_account = &process_instruction_deactivate_delinquent(
        &stake_address,
        &stake_account,
        &vote_account,
        &reference_vote_account,
        Ok(()),
    )[0];

    assert_eq!(
        stake_from(post_stake_account)
            .unwrap()
            .delegation
            .deactivation_epoch,
        current_epoch
    );

    // `reference_vote_account` has consistently voted and `vote_account` has not voted for the
    // last `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`. Try to deactivate an unrelated stake
    // account.  Instruction will fail
    let unrelated_vote_address = Pubkey::new_unique();
    let unrelated_stake_address = Pubkey::new_unique();
    let mut unrelated_stake_account = stake_account.clone();
    assert_ne!(unrelated_vote_address, vote_address);
    unrelated_stake_account
        .serialize_data(&StakeStateV2::Stake(
            Meta::default(),
            new_stake(
                1, /* stake */
                &unrelated_vote_address,
                &VoteState::default(),
                1, /* activation_epoch */
            ),
            StakeFlags::empty(),
        ))
        .unwrap();

    process_instruction_deactivate_delinquent(
        &unrelated_stake_address,
        &unrelated_stake_account,
        &vote_account,
        &reference_vote_account,
        Err(StakeError::VoteAddressMismatch.into()),
    );

    // `reference_vote_account` has consistently voted and `vote_account` voted once
    // `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION` ago.
    // Instruction will succeed
    let mut vote_state = VoteState::default();
    vote_state.increment_credits(
        current_epoch - MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch,
        1,
    );
    vote_account
        .serialize_data(&VoteStateVersions::new_current(vote_state))
        .unwrap();
    process_instruction_deactivate_delinquent(
        &stake_address,
        &stake_account,
        &vote_account,
        &reference_vote_account,
        Ok(()),
    );

    // `reference_vote_account` has consistently voted and `vote_account` voted once
    // `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION` - 1 epochs ago
    // Instruction will fail
    let mut vote_state = VoteState::default();
    vote_state.increment_credits(
        current_epoch - (MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION - 1) as Epoch,
        1,
    );
    vote_account
        .serialize_data(&VoteStateVersions::new_current(vote_state))
        .unwrap();
    process_instruction_deactivate_delinquent(
        &stake_address,
        &stake_account,
        &vote_account,
        &reference_vote_account,
        Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into()),
    );
}
