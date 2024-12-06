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
            self, config as stake_config,
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
            epoch_rewards::{self, EpochRewards},
            epoch_schedule::{self, EpochSchedule},
            rent::{self, Rent},
            rewards,
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
    std::{
        collections::{HashMap, HashSet},
        fs,
        str::FromStr,
        sync::Arc,
    },
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

fn create_default_account() -> AccountSharedData {
    AccountSharedData::new(0, 0, &Pubkey::new_unique())
}

fn create_default_stake_account() -> AccountSharedData {
    AccountSharedData::new(0, 0, &id())
}

fn invalid_stake_state_pubkey() -> Pubkey {
    Pubkey::from_str("BadStake11111111111111111111111111111111111").unwrap()
}

fn invalid_vote_state_pubkey() -> Pubkey {
    Pubkey::from_str("BadVote111111111111111111111111111111111111").unwrap()
}

fn spoofed_stake_state_pubkey() -> Pubkey {
    Pubkey::from_str("SpoofedStake1111111111111111111111111111111").unwrap()
}

fn spoofed_stake_program_id() -> Pubkey {
    Pubkey::from_str("Spoofed111111111111111111111111111111111111").unwrap()
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

fn get_default_transaction_accounts(instruction: &Instruction) -> Vec<(Pubkey, AccountSharedData)> {
    let mut pubkeys: HashSet<Pubkey> = instruction
        .accounts
        .iter()
        .map(|meta| meta.pubkey)
        .collect();
    pubkeys.insert(clock::id());
    pubkeys.insert(epoch_schedule::id());
    pubkeys.insert(stake_history::id());
    #[allow(deprecated)]
    pubkeys
        .iter()
        .map(|pubkey| {
            (
                *pubkey,
                if clock::check_id(pubkey) {
                    create_account_shared_data_for_test(&clock::Clock::default())
                } else if rewards::check_id(pubkey) {
                    create_account_shared_data_for_test(&rewards::Rewards::new(0.0))
                } else if stake_history::check_id(pubkey) {
                    create_account_shared_data_for_test(&StakeHistory::default())
                } else if stake_config::check_id(pubkey) {
                    config::create_account(0, &stake_config::Config::default())
                } else if epoch_schedule::check_id(pubkey) {
                    create_account_shared_data_for_test(&EpochSchedule::default())
                } else if rent::check_id(pubkey) {
                    create_account_shared_data_for_test(&Rent::default())
                } else if *pubkey == invalid_stake_state_pubkey() {
                    AccountSharedData::new(0, 0, &id())
                } else if *pubkey == invalid_vote_state_pubkey() {
                    AccountSharedData::new(0, 0, &solana_vote_program::id())
                } else if *pubkey == spoofed_stake_state_pubkey() {
                    AccountSharedData::new(0, 0, &spoofed_stake_program_id())
                } else {
                    AccountSharedData::new(0, 0, &id())
                },
            )
        })
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

mod config {
    #[allow(deprecated)]
    use solana_sdk::stake::config::{self, *};
    use {
        bincode::deserialize,
        solana_config_program::{create_config_account, get_config_data},
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount, WritableAccount},
            genesis_config::GenesisConfig,
            transaction_context::BorrowedAccount,
        },
    };

    #[allow(deprecated)]
    pub fn from(account: &BorrowedAccount) -> Option<Config> {
        get_config_data(account.get_data())
            .ok()
            .and_then(|data| deserialize(data).ok())
    }

    #[allow(deprecated)]
    pub fn create_account(lamports: u64, config: &Config) -> AccountSharedData {
        create_config_account(vec![], config, lamports)
    }

    #[allow(deprecated)]
    pub fn add_genesis_account(genesis_config: &mut GenesisConfig) -> u64 {
        let mut account = create_config_account(vec![], &Config::default(), 0);
        let lamports = genesis_config.rent.minimum_balance(account.data().len());

        account.set_lamports(lamports.max(1));

        genesis_config.add_account(config::id(), account);

        lamports
    }
}

// Ensure that the correct errors are returned when processing instructions
//
// The GetMinimumDelegation instruction does not take any accounts; so when it was added,
// `process_instruction()` needed to be updated to *not* need a stake account passed in, which
// changes the error *ordering* conditions.
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_process_instruction_error_ordering(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let rent_address = rent::id();
    let rent_account = create_account_shared_data_for_test(&rent);

    let good_stake_address = Pubkey::new_unique();
    let good_stake_account =
        AccountSharedData::new(rent_exempt_reserve, StakeStateV2::size_of(), &id());
    let good_instruction = instruction::initialize(
        &good_stake_address,
        &Authorized::auto(&good_stake_address),
        &Lockup::default(),
    );
    let good_transaction_accounts = vec![
        (good_stake_address, good_stake_account),
        (rent_address, rent_account),
    ];
    let good_instruction_accounts = vec![
        AccountMeta {
            pubkey: good_stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: rent_address,
            is_signer: false,
            is_writable: false,
        },
    ];
    let good_accounts = (good_transaction_accounts, good_instruction_accounts);

    // The instruction data needs to deserialize to a bogus StakeInstruction.  We likely never
    // will have `usize::MAX`-number of instructions, so this should be a safe constant to
    // always map to an invalid stake instruction.
    let bad_instruction = Instruction::new_with_bincode(id(), &usize::MAX, Vec::default());
    let bad_transaction_accounts = Vec::default();
    let bad_instruction_accounts = Vec::default();
    let bad_accounts = (bad_transaction_accounts, bad_instruction_accounts);

    for (instruction, (transaction_accounts, instruction_accounts), expected_result) in [
        (&good_instruction, &good_accounts, Ok(())),
        (
            &bad_instruction,
            &good_accounts,
            Err(ProgramError::InvalidInstructionData),
        ),
        (
            &good_instruction,
            &bad_accounts,
            Err(ProgramError::NotEnoughAccountKeys),
        ),
        (
            &bad_instruction,
            &bad_accounts,
            Err(ProgramError::InvalidInstructionData),
        ),
    ] {
        process_instruction(
            &mollusk,
            &instruction.data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            expected_result,
        );
    }
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

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_process_instruction_with_epoch_rewards_active(mollusk: Mollusk) {
    let process_instruction_as_one_arg = |mollusk: &Mollusk,
                                          instruction: &Instruction,
                                          expected_result: Result<(), ProgramError>|
     -> Vec<AccountSharedData> {
        let mut transaction_accounts = get_default_transaction_accounts(instruction);

        // Initialize EpochRewards sysvar account
        let epoch_rewards_sysvar = EpochRewards {
            active: true,
            ..EpochRewards::default()
        };
        transaction_accounts.push((
            epoch_rewards::id(),
            create_account_shared_data_for_test(&epoch_rewards_sysvar),
        ));

        process_instruction(
            &mollusk,
            &instruction.data,
            transaction_accounts,
            instruction.accounts.clone(),
            expected_result,
        )
    };

    process_instruction_as_one_arg(
        &mollusk,
        &instruction::initialize(
            &Pubkey::new_unique(),
            &Authorized::default(),
            &Lockup::default(),
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::authorize(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            StakeAuthorize::Staker,
            None,
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::delegate_stake(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &invalid_vote_state_pubkey(),
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::split(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
            &invalid_stake_state_pubkey(),
        )[2],
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::withdraw(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
            None,
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::deactivate_stake(&Pubkey::new_unique(), &Pubkey::new_unique()),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::set_lockup(
            &Pubkey::new_unique(),
            &LockupArgs::default(),
            &Pubkey::new_unique(),
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::merge(
            &Pubkey::new_unique(),
            &invalid_stake_state_pubkey(),
            &Pubkey::new_unique(),
        )[0],
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::authorize_with_seed(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            "seed".to_string(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            StakeAuthorize::Staker,
            None,
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );

    process_instruction_as_one_arg(
        &mollusk,
        &instruction::initialize_checked(&Pubkey::new_unique(), &Authorized::default()),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::authorize_checked(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            StakeAuthorize::Staker,
            None,
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::authorize_checked_with_seed(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            "seed".to_string(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            StakeAuthorize::Staker,
            None,
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::set_lockup_checked(
            &Pubkey::new_unique(),
            &LockupArgs::default(),
            &Pubkey::new_unique(),
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::deactivate_delinquent_stake(
            &Pubkey::new_unique(),
            &invalid_vote_state_pubkey(),
            &Pubkey::new_unique(),
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::move_stake(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::move_lamports(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
        ),
        Err(StakeError::EpochRewardsActive.into()),
    );

    // Only GetMinimumDelegation should not return StakeError::EpochRewardsActive
    process_instruction_as_one_arg(&mollusk, &instruction::get_minimum_delegation(), Ok(()));
}
