#![allow(clippy::arithmetic_side_effects)]

use {
    assert_matches::assert_matches,
    bincode::serialize,
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, ReadableAccount, WritableAccount},
    solana_program_runtime::loaded_programs::ProgramCacheEntryOwner,
    solana_sdk::{
        account::create_account_shared_data_for_test,
        account_utils::StateMut,
        feature_set::stake_raise_minimum_delegation_to_1_sol,
        instruction::{AccountMeta, Instruction},
        program_error::ProgramError,
        pubkey::Pubkey,
        stake::{
            config as stake_config,
            instruction::{
                self, authorize_checked, authorize_checked_with_seed, initialize_checked,
                set_lockup_checked, AuthorizeCheckedWithSeedArgs, AuthorizeWithSeedArgs,
                LockupArgs, StakeError, StakeInstruction,
            },
            stake_flags::StakeFlags,
            state::{
                warmup_cooldown_rate, Authorized, Delegation, Lockup, Meta, Stake, StakeAuthorize,
                StakeStateV2,
            },
            MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION,
        },
        stake_history::{Epoch, StakeHistoryEntry},
        system_program,
        sysvar::{
            clock::{self, Clock},
            epoch_rewards::{self, EpochRewards},
            epoch_schedule::{self, EpochSchedule},
            rent::{self, Rent},
            rewards,
            stake_history::{self, StakeHistory},
        },
    },
    solana_stake_program::{get_minimum_delegation, id},
    solana_vote_program::{
        self,
        vote_state::{self, VoteState, VoteStateVersions},
    },
    std::{collections::HashSet, str::FromStr},
    test_case::test_case,
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

trait IsBpf {
    fn is_bpf(&self) -> bool;
}
impl IsBpf for Mollusk {
    fn is_bpf(&self) -> bool {
        self.program_cache
            .load_program(&id())
            .unwrap()
            .account_owner
            != ProgramCacheEntryOwner::NativeLoader
    }
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
    mut transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
    instruction_accounts: Vec<AccountMeta>,
    expected_result: Result<(), ProgramError>,
) -> Vec<AccountSharedData> {
    for ixn_key in instruction_accounts.iter().map(|meta| meta.pubkey) {
        if !transaction_accounts
            .iter()
            .any(|(txn_key, _)| *txn_key == ixn_key)
        {
            transaction_accounts.push((ixn_key, AccountSharedData::default()));
        }
    }

    let instruction = Instruction {
        program_id: id(),
        accounts: instruction_accounts,
        data: instruction_data.to_vec(),
    };

    let check = match expected_result {
        Ok(()) => Check::success(),
        Err(e) => Check::err(e),
    };

    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &transaction_accounts
            .into_iter()
            .map(|(key, account)| (key, account.into()))
            .collect::<Vec<_>>(),
        &[check],
    );

    result
        .resulting_accounts
        .into_iter()
        .map(|(_, account)| account.into())
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

fn process_instruction_as_one_arg(
    mollusk: &Mollusk,
    instruction: &Instruction,
    expected_result: Result<(), ProgramError>,
) -> Vec<AccountSharedData> {
    let transaction_accounts = get_default_transaction_accounts(instruction);
    process_instruction(
        mollusk,
        &instruction.data,
        transaction_accounts,
        instruction.accounts.clone(),
        expected_result,
    )
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

fn authorized_from(account: &AccountSharedData) -> Option<Authorized> {
    from(account).and_then(|state: StakeStateV2| state.authorized())
}

fn just_stake(meta: Meta, stake: u64) -> StakeStateV2 {
    StakeStateV2::Stake(
        meta,
        Stake {
            delegation: Delegation {
                stake,
                ..Delegation::default()
            },
            ..Stake::default()
        },
        StakeFlags::empty(),
    )
}

fn get_active_stake_for_tests(
    stake_accounts: &[AccountSharedData],
    clock: &Clock,
    stake_history: &StakeHistory,
) -> u64 {
    let mut active_stake = 0;
    for account in stake_accounts {
        if let StakeStateV2::Stake(_meta, stake, _stake_flags) = account.state().unwrap() {
            let stake_status = stake.delegation.stake_activating_and_deactivating(
                clock.epoch,
                stake_history,
                None,
            );
            active_stake += stake_status.effective;
        }
    }
    active_stake
}

fn create_empty_stake_history_for_test() -> AccountSharedData {
    AccountSharedData::create(1, vec![0; 8], solana_program::sysvar::id(), false, u64::MAX)
}

fn new_stake_history_entry<'a, I>(
    epoch: Epoch,
    stakes: I,
    history: &StakeHistory,
    new_rate_activation_epoch: Option<Epoch>,
) -> StakeHistoryEntry
where
    I: Iterator<Item = &'a Delegation>,
{
    stakes.fold(StakeHistoryEntry::default(), |sum, stake| {
        sum + stake.stake_activating_and_deactivating(epoch, history, new_rate_activation_epoch)
    })
}

fn create_stake_history_from_delegations(
    bootstrap: Option<u64>,
    epochs: std::ops::Range<Epoch>,
    delegations: &[Delegation],
    new_rate_activation_epoch: Option<Epoch>,
) -> StakeHistory {
    let mut stake_history = StakeHistory::default();

    let bootstrap_delegation = if let Some(bootstrap) = bootstrap {
        vec![Delegation {
            activation_epoch: u64::MAX,
            stake: bootstrap,
            ..Delegation::default()
        }]
    } else {
        vec![]
    };

    for epoch in epochs {
        let entry = new_stake_history_entry(
            epoch,
            delegations.iter().chain(bootstrap_delegation.iter()),
            &stake_history,
            new_rate_activation_epoch,
        );
        stake_history.add(epoch, entry);
    }

    stake_history
}

mod config {
    #[allow(deprecated)]
    use {
        solana_config_program::create_config_account,
        solana_sdk::{account::AccountSharedData, stake::config::Config},
    };

    #[allow(deprecated)]
    pub fn create_account(lamports: u64, config: &Config) -> AccountSharedData {
        create_config_account(vec![], config, lamports)
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_process_instruction(mollusk: Mollusk) {
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::initialize(
            &Pubkey::new_unique(),
            &Authorized::default(),
            &Lockup::default(),
        ),
        Err(ProgramError::InvalidAccountData),
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
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::split(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
            &invalid_stake_state_pubkey(),
        )[2],
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::merge(
            &Pubkey::new_unique(),
            &invalid_stake_state_pubkey(),
            &Pubkey::new_unique(),
        )[0],
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::split_with_seed(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
            &invalid_stake_state_pubkey(),
            &Pubkey::new_unique(),
            "seed",
        )[1],
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::delegate_stake(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &invalid_vote_state_pubkey(),
        ),
        Err(ProgramError::InvalidAccountData),
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
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::deactivate_stake(&Pubkey::new_unique(), &Pubkey::new_unique()),
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::set_lockup(
            &Pubkey::new_unique(),
            &LockupArgs::default(),
            &Pubkey::new_unique(),
        ),
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::deactivate_delinquent_stake(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &invalid_vote_state_pubkey(),
        ),
        Err(ProgramError::IncorrectProgramId),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::deactivate_delinquent_stake(
            &Pubkey::new_unique(),
            &invalid_vote_state_pubkey(),
            &Pubkey::new_unique(),
        ),
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::deactivate_delinquent_stake(
            &Pubkey::new_unique(),
            &invalid_vote_state_pubkey(),
            &invalid_vote_state_pubkey(),
        ),
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::move_stake(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
        ),
        Err(ProgramError::InvalidAccountData),
    );
    process_instruction_as_one_arg(
        &mollusk,
        &instruction::move_lamports(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100,
        ),
        Err(ProgramError::InvalidAccountData),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_process_instruction_decode_bail(mollusk: Mollusk) {
    // these will not call stake_state, have bogus contents
    let stake_address = Pubkey::new_unique();
    let stake_account = create_default_stake_account();
    let rent_address = rent::id();
    let rent = Rent::default();
    let rent_account = create_account_shared_data_for_test(&rent);
    let rewards_address = rewards::id();
    let rewards_account = create_account_shared_data_for_test(&rewards::Rewards::new(0.0));
    let stake_history_address = stake_history::id();
    let stake_history_account = create_account_shared_data_for_test(&StakeHistory::default());
    let vote_address = Pubkey::new_unique();
    let vote_account = AccountSharedData::new(0, 0, &solana_vote_program::id());
    let clock_address = clock::id();
    let clock_account = create_account_shared_data_for_test(&clock::Clock::default());
    #[allow(deprecated)]
    let config_address = stake_config::id();
    #[allow(deprecated)]
    let config_account = config::create_account(0, &stake_config::Config::default());
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let minimum_delegation = crate::get_minimum_delegation();
    let withdrawal_amount = rent_exempt_reserve + minimum_delegation;

    // gets the "is_empty()" check
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Initialize(
            Authorized::default(),
            Lockup::default(),
        ))
        .unwrap(),
        Vec::new(),
        Vec::new(),
        Err(ProgramError::NotEnoughAccountKeys),
    );

    // no account for rent
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Initialize(
            Authorized::default(),
            Lockup::default(),
        ))
        .unwrap(),
        vec![(stake_address, stake_account.clone())],
        vec![AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        }],
        Err(ProgramError::NotEnoughAccountKeys),
    );

    // fails to deserialize stake state
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Initialize(
            Authorized::default(),
            Lockup::default(),
        ))
        .unwrap(),
        vec![
            (stake_address, stake_account.clone()),
            (rent_address, rent_account),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rent_address,
                is_signer: false,
                is_writable: false,
            },
        ],
        Err(ProgramError::InvalidAccountData),
    );

    // gets the first check in delegate, wrong number of accounts
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        vec![(stake_address, stake_account.clone())],
        vec![AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        }],
        Err(ProgramError::NotEnoughAccountKeys),
    );

    // gets the sub-check for number of args
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        vec![(stake_address, stake_account.clone())],
        vec![AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        }],
        Err(ProgramError::NotEnoughAccountKeys),
    );

    // gets the check non-deserialize-able account in delegate_stake
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        vec![
            (stake_address, stake_account.clone()),
            (vote_address, vote_account.clone()),
            (clock_address, clock_account),
            (stake_history_address, stake_history_account.clone()),
            (config_address, config_account),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: config_address,
                is_signer: false,
                is_writable: false,
            },
        ],
        Err(ProgramError::InvalidAccountData),
    );

    // Tests 3rd keyed account is of correct type (Clock instead of rewards) in withdraw
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(withdrawal_amount)).unwrap(),
        vec![
            (stake_address, stake_account.clone()),
            (vote_address, vote_account.clone()),
            (rewards_address, rewards_account.clone()),
            (stake_history_address, stake_history_account),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: rewards_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
        ],
        Err(ProgramError::InvalidArgument),
    );

    // Tests correct number of accounts are provided in withdraw
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(withdrawal_amount)).unwrap(),
        vec![(stake_address, stake_account.clone())],
        vec![AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        }],
        Err(ProgramError::NotEnoughAccountKeys),
    );

    // Tests 2nd keyed account is of correct type (Clock instead of rewards) in deactivate
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        vec![
            (stake_address, stake_account.clone()),
            (rewards_address, rewards_account),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rewards_address,
                is_signer: false,
                is_writable: false,
            },
        ],
        Err(ProgramError::InvalidArgument),
    );

    // Tests correct number of accounts are provided in deactivate
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        Vec::new(),
        Vec::new(),
        Err(ProgramError::NotEnoughAccountKeys),
    );

    // Tests correct number of accounts are provided in deactivate_delinquent
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DeactivateDelinquent).unwrap(),
        Vec::new(),
        Vec::new(),
        Err(ProgramError::NotEnoughAccountKeys),
    );
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DeactivateDelinquent).unwrap(),
        vec![(stake_address, stake_account.clone())],
        vec![AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        }],
        Err(ProgramError::NotEnoughAccountKeys),
    );
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DeactivateDelinquent).unwrap(),
        vec![(stake_address, stake_account), (vote_address, vote_account)],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
        ],
        Err(ProgramError::NotEnoughAccountKeys),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_checked_instructions(mollusk: Mollusk) {
    let stake_address = Pubkey::new_unique();
    let staker = Pubkey::new_unique();
    let staker_account = create_default_account();
    let withdrawer = Pubkey::new_unique();
    let withdrawer_account = create_default_account();
    let authorized_address = Pubkey::new_unique();
    let authorized_account = create_default_account();
    let new_authorized_account = create_default_account();
    let clock_address = clock::id();
    let clock_account = create_account_shared_data_for_test(&Clock::default());
    let custodian = Pubkey::new_unique();
    let custodian_account = create_default_account();
    let rent = Rent::default();
    let rent_address = rent::id();
    let rent_account = create_account_shared_data_for_test(&rent);
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let minimum_delegation = crate::get_minimum_delegation();

    // Test InitializeChecked with non-signing withdrawer
    let mut instruction = initialize_checked(&stake_address, &Authorized { staker, withdrawer });
    instruction.accounts[3] = AccountMeta::new_readonly(withdrawer, false);
    process_instruction_as_one_arg(
        &mollusk,
        &instruction,
        Err(ProgramError::MissingRequiredSignature),
    );

    // Test InitializeChecked with withdrawer signer
    let stake_account = AccountSharedData::new(
        rent_exempt_reserve + minimum_delegation,
        StakeStateV2::size_of(),
        &id(),
    );
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::InitializeChecked).unwrap(),
        vec![
            (stake_address, stake_account),
            (rent_address, rent_account),
            (staker, staker_account),
            (withdrawer, withdrawer_account.clone()),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rent_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: staker,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: withdrawer,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );

    // Test AuthorizeChecked with non-signing authority
    let mut instruction = authorize_checked(
        &stake_address,
        &authorized_address,
        &staker,
        StakeAuthorize::Staker,
        None,
    );
    instruction.accounts[3] = AccountMeta::new_readonly(staker, false);
    process_instruction_as_one_arg(
        &mollusk,
        &instruction,
        Err(ProgramError::MissingRequiredSignature),
    );

    let mut instruction = authorize_checked(
        &stake_address,
        &authorized_address,
        &withdrawer,
        StakeAuthorize::Withdrawer,
        None,
    );
    instruction.accounts[3] = AccountMeta::new_readonly(withdrawer, false);
    process_instruction_as_one_arg(
        &mollusk,
        &instruction,
        Err(ProgramError::MissingRequiredSignature),
    );

    // Test AuthorizeChecked with authority signer
    let stake_account = AccountSharedData::new_data_with_space(
        42,
        &StakeStateV2::Initialized(Meta::auto(&authorized_address)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::AuthorizeChecked(StakeAuthorize::Staker)).unwrap(),
        vec![
            (stake_address, stake_account.clone()),
            (clock_address, clock_account.clone()),
            (authorized_address, authorized_account.clone()),
            (staker, new_authorized_account.clone()),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authorized_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: staker,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );

    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::AuthorizeChecked(
            StakeAuthorize::Withdrawer,
        ))
        .unwrap(),
        vec![
            (stake_address, stake_account),
            (clock_address, clock_account.clone()),
            (authorized_address, authorized_account.clone()),
            (withdrawer, new_authorized_account.clone()),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authorized_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: withdrawer,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );

    // Test AuthorizeCheckedWithSeed with non-signing authority
    let authorized_owner = Pubkey::new_unique();
    let seed = "test seed";
    let address_with_seed =
        Pubkey::create_with_seed(&authorized_owner, seed, &authorized_owner).unwrap();
    let mut instruction = authorize_checked_with_seed(
        &stake_address,
        &authorized_owner,
        seed.to_string(),
        &authorized_owner,
        &staker,
        StakeAuthorize::Staker,
        None,
    );
    instruction.accounts[3] = AccountMeta::new_readonly(staker, false);
    process_instruction_as_one_arg(
        &mollusk,
        &instruction,
        Err(ProgramError::MissingRequiredSignature),
    );

    let mut instruction = authorize_checked_with_seed(
        &stake_address,
        &authorized_owner,
        seed.to_string(),
        &authorized_owner,
        &staker,
        StakeAuthorize::Withdrawer,
        None,
    );
    instruction.accounts[3] = AccountMeta::new_readonly(staker, false);
    process_instruction_as_one_arg(
        &mollusk,
        &instruction,
        Err(ProgramError::MissingRequiredSignature),
    );

    // Test AuthorizeCheckedWithSeed with authority signer
    let stake_account = AccountSharedData::new_data_with_space(
        42,
        &StakeStateV2::Initialized(Meta::auto(&address_with_seed)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::AuthorizeCheckedWithSeed(
            AuthorizeCheckedWithSeedArgs {
                stake_authorize: StakeAuthorize::Staker,
                authority_seed: seed.to_string(),
                authority_owner: authorized_owner,
            },
        ))
        .unwrap(),
        vec![
            (address_with_seed, stake_account.clone()),
            (authorized_owner, authorized_account.clone()),
            (clock_address, clock_account.clone()),
            (staker, new_authorized_account.clone()),
        ],
        vec![
            AccountMeta {
                pubkey: address_with_seed,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: authorized_owner,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: staker,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );

    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::AuthorizeCheckedWithSeed(
            AuthorizeCheckedWithSeedArgs {
                stake_authorize: StakeAuthorize::Withdrawer,
                authority_seed: seed.to_string(),
                authority_owner: authorized_owner,
            },
        ))
        .unwrap(),
        vec![
            (address_with_seed, stake_account),
            (authorized_owner, authorized_account),
            (clock_address, clock_account.clone()),
            (withdrawer, new_authorized_account),
        ],
        vec![
            AccountMeta {
                pubkey: address_with_seed,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: authorized_owner,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: withdrawer,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );

    // Test SetLockupChecked with non-signing lockup custodian
    let mut instruction = set_lockup_checked(
        &stake_address,
        &LockupArgs {
            unix_timestamp: None,
            epoch: Some(1),
            custodian: Some(custodian),
        },
        &withdrawer,
    );
    instruction.accounts[2] = AccountMeta::new_readonly(custodian, false);
    process_instruction_as_one_arg(
        &mollusk,
        &instruction,
        Err(ProgramError::MissingRequiredSignature),
    );

    // Test SetLockupChecked with lockup custodian signer
    let stake_account = AccountSharedData::new_data_with_space(
        42,
        &StakeStateV2::Initialized(Meta::auto(&withdrawer)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    process_instruction(
        &mollusk,
        &instruction.data,
        vec![
            (clock_address, clock_account),
            (stake_address, stake_account),
            (withdrawer, withdrawer_account),
            (custodian, custodian_account),
        ],
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: withdrawer,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: custodian,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_initialize(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_lamports = rent_exempt_reserve;
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_account = AccountSharedData::new(stake_lamports, StakeStateV2::size_of(), &id());
    let custodian_address = solana_sdk::pubkey::new_rand();
    let lockup = Lockup {
        epoch: 1,
        unix_timestamp: 0,
        custodian: custodian_address,
    };
    let instruction_data = serialize(&StakeInstruction::Initialize(
        Authorized::auto(&stake_address),
        lockup,
    ))
    .unwrap();
    let mut transaction_accounts = vec![
        (stake_address, stake_account.clone()),
        (rent::id(), create_account_shared_data_for_test(&rent)),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: rent::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    // should pass
    let accounts = process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    // check that we see what we expect
    assert_eq!(
        from(&accounts[0]).unwrap(),
        StakeStateV2::Initialized(Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve,
            lockup,
        }),
    );

    // 2nd time fails, can't move it from anything other than uninit->init
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InvalidAccountData),
    );
    transaction_accounts[0] = (stake_address, stake_account);

    // not enough balance for rent
    transaction_accounts[1] = (
        rent::id(),
        create_account_shared_data_for_test(&Rent {
            lamports_per_byte_year: rent.lamports_per_byte_year + 1,
            ..rent
        }),
    );
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InsufficientFunds),
    );

    // incorrect account sizes
    let stake_account = AccountSharedData::new(stake_lamports, StakeStateV2::size_of() + 1, &id());
    transaction_accounts[0] = (stake_address, stake_account);
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InvalidAccountData),
    );

    let stake_account = AccountSharedData::new(stake_lamports, StakeStateV2::size_of() - 1, &id());
    transaction_accounts[0] = (stake_address, stake_account);
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::InvalidAccountData),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_authorize(mollusk: Mollusk) {
    let authority_address = solana_sdk::pubkey::new_rand();
    let authority_address_2 = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_lamports = 42;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::default(),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let to_address = solana_sdk::pubkey::new_rand();
    let to_account = AccountSharedData::new(1, 0, &system_program::id());
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (to_address, to_account),
        (authority_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authority_address,
            is_signer: false,
            is_writable: false,
        },
    ];

    // should fail, uninit
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InvalidAccountData),
    );

    // should pass
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Initialized(Meta::auto(&stake_address)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    transaction_accounts[0] = (stake_address, stake_account);
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Withdrawer,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    if let StakeStateV2::Initialized(Meta { authorized, .. }) = from(&accounts[0]).unwrap() {
        assert_eq!(authorized.staker, authority_address);
        assert_eq!(authorized.withdrawer, authority_address);
    } else {
        panic!();
    }

    // A second authorization signed by the stake account should fail
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address_2,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );

    // Test a second authorization by the new authority_address
    instruction_accounts[0].is_signer = false;
    instruction_accounts[2].is_signer = true;
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address_2,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    if let StakeStateV2::Initialized(Meta { authorized, .. }) = from(&accounts[0]).unwrap() {
        assert_eq!(authorized.staker, authority_address_2);
    } else {
        panic!();
    }

    // Test a successful action by the currently authorized withdrawer
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: to_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authority_address,
            is_signer: true,
            is_writable: false,
        },
    ];
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    assert_eq!(from(&accounts[0]).unwrap(), StakeStateV2::Uninitialized);

    // Test that withdrawal to account fails without authorized withdrawer
    instruction_accounts[4].is_signer = false;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::MissingRequiredSignature),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_authorize_override(mollusk: Mollusk) {
    let authority_address = solana_sdk::pubkey::new_rand();
    let mallory_address = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_lamports = 42;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Initialized(Meta::auto(&stake_address)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (authority_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authority_address,
            is_signer: false,
            is_writable: false,
        },
    ];

    // Authorize a staker pubkey and move the withdrawer key into cold storage.
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // Attack! The stake key (a hot key) is stolen and used to authorize a new staker.
    instruction_accounts[0].is_signer = false;
    instruction_accounts[2].is_signer = true;
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            mallory_address,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // Verify the original staker no longer has access.
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );

    // Verify the withdrawer (pulled from cold storage) can save the day.
    instruction_accounts[0].is_signer = true;
    instruction_accounts[2].is_signer = false;
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Withdrawer,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // Attack! Verify the staker cannot be used to authorize a withdraw.
    instruction_accounts[0].is_signer = false;
    instruction_accounts[2] = AccountMeta {
        pubkey: mallory_address,
        is_signer: true,
        is_writable: false,
    };
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Withdrawer,
        ))
        .unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::MissingRequiredSignature),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_authorize_with_seed(mollusk: Mollusk) {
    let authority_base_address = solana_sdk::pubkey::new_rand();
    let authority_address = solana_sdk::pubkey::new_rand();
    let seed = "42";
    let stake_address = Pubkey::create_with_seed(&authority_base_address, seed, &id()).unwrap();
    let stake_lamports = 42;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Initialized(Meta::auto(&stake_address)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (authority_base_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: authority_base_address,
            is_signer: true,
            is_writable: false,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    // Wrong seed
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::AuthorizeWithSeed(
            AuthorizeWithSeedArgs {
                new_authorized_pubkey: authority_address,
                stake_authorize: StakeAuthorize::Staker,
                authority_seed: "".to_string(),
                authority_owner: id(),
            },
        ))
        .unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );

    // Wrong base
    instruction_accounts[1].pubkey = authority_address;
    let instruction_data = serialize(&StakeInstruction::AuthorizeWithSeed(
        AuthorizeWithSeedArgs {
            new_authorized_pubkey: authority_address,
            stake_authorize: StakeAuthorize::Staker,
            authority_seed: seed.to_string(),
            authority_owner: id(),
        },
    ))
    .unwrap();
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );
    instruction_accounts[1].pubkey = authority_base_address;

    // Set stake authority
    let accounts = process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // Set withdraw authority
    let instruction_data = serialize(&StakeInstruction::AuthorizeWithSeed(
        AuthorizeWithSeedArgs {
            new_authorized_pubkey: authority_address,
            stake_authorize: StakeAuthorize::Withdrawer,
            authority_seed: seed.to_string(),
            authority_owner: id(),
        },
    ))
    .unwrap();
    let accounts = process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // No longer withdraw authority
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::MissingRequiredSignature),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_authorize_delegated_stake(mollusk: Mollusk) {
    let authority_address = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = minimum_delegation;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Initialized(Meta::auto(&stake_address)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let vote_address = solana_sdk::pubkey::new_rand();
    let vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    let vote_address_2 = solana_sdk::pubkey::new_rand();
    let mut vote_account_2 =
        vote_state::create_account(&vote_address_2, &solana_sdk::pubkey::new_rand(), 0, 100);
    vote_account_2
        .set_state(&VoteStateVersions::new_current(VoteState::default()))
        .unwrap();
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (vote_address, vote_account),
        (vote_address_2, vote_account_2),
        (
            authority_address,
            AccountSharedData::new(42, 0, &system_program::id()),
        ),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    #[allow(deprecated)]
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: vote_address,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_config::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    // delegate stake
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // deactivate, so we can re-delegate
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // authorize
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authority_address,
            StakeAuthorize::Staker,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    assert_eq!(
        authorized_from(&accounts[0]).unwrap().staker,
        authority_address
    );

    // Random other account should fail
    instruction_accounts[0].is_signer = false;
    instruction_accounts[1].pubkey = vote_address_2;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );

    // Authorized staker should succeed
    instruction_accounts.push(AccountMeta {
        pubkey: authority_address,
        is_signer: true,
        is_writable: false,
    });
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts,
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    assert_eq!(
        stake_from(&accounts[0]).unwrap().delegation.voter_pubkey,
        vote_address_2,
    );

    // Test another staking action
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts,
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_delegate(mollusk: Mollusk) {
    let mut vote_state = VoteState::default();
    for i in 0..1000 {
        vote_state::process_slot_vote_unchecked(&mut vote_state, i);
    }
    let vote_state_credits = vote_state.credits();
    let vote_address = solana_sdk::pubkey::new_rand();
    let vote_address_2 = solana_sdk::pubkey::new_rand();
    let mut vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    let mut vote_account_2 =
        vote_state::create_account(&vote_address_2, &solana_sdk::pubkey::new_rand(), 0, 100);
    vote_account
        .set_state(&VoteStateVersions::new_current(vote_state.clone()))
        .unwrap();
    vote_account_2
        .set_state(&VoteStateVersions::new_current(vote_state))
        .unwrap();
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = minimum_delegation;
    let stake_address = solana_sdk::pubkey::new_rand();
    let mut stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Initialized(Meta {
            authorized: Authorized {
                staker: stake_address,
                withdrawer: stake_address,
            },
            ..Meta::default()
        }),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let mut clock = Clock {
        epoch: 1,
        ..Clock::default()
    };
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account.clone()),
        (vote_address, vote_account),
        (vote_address_2, vote_account_2.clone()),
        (clock::id(), create_account_shared_data_for_test(&clock)),
        (stake_history::id(), create_empty_stake_history_for_test()),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    #[allow(deprecated)]
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: vote_address,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_config::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    // should fail, unsigned stake account
    instruction_accounts[0].is_signer = false;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );
    instruction_accounts[0].is_signer = true;

    // should pass
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    // verify that delegate() looks right, compare against hand-rolled
    assert_eq!(
        stake_from(&accounts[0]).unwrap(),
        Stake {
            delegation: Delegation {
                voter_pubkey: vote_address,
                stake: stake_lamports,
                activation_epoch: clock.epoch,
                deactivation_epoch: u64::MAX,
                ..Delegation::default()
            },
            credits_observed: vote_state_credits,
        }
    );

    // verify that delegate fails as stake is active and not deactivating
    clock.epoch += 1;
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(StakeError::TooSoonToRedelegate.into()),
    );

    // deactivate
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );

    // verify that delegate to a different vote account fails
    // during deactivation
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    instruction_accounts[1].pubkey = vote_address_2;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(StakeError::TooSoonToRedelegate.into()),
    );
    instruction_accounts[1].pubkey = vote_address;

    // verify that delegate succeeds to same vote account
    // when stake is deactivating
    let accounts_2 = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    // verify that deactivation has been cleared
    let stake = stake_from(&accounts_2[0]).unwrap();
    assert_eq!(stake.delegation.deactivation_epoch, u64::MAX);

    // verify that delegate to a different vote account fails
    // if stake is still active
    transaction_accounts[0] = (stake_address, accounts_2[0].clone());
    instruction_accounts[1].pubkey = vote_address_2;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(StakeError::TooSoonToRedelegate.into()),
    );

    // without stake history, cool down is instantaneous
    clock.epoch += 1;
    transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));

    // verify that delegate can be called to new vote account, 2nd is redelegate
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    instruction_accounts[1].pubkey = vote_address;
    // verify that delegate() looks right, compare against hand-rolled
    assert_eq!(
        stake_from(&accounts[0]).unwrap(),
        Stake {
            delegation: Delegation {
                voter_pubkey: vote_address_2,
                stake: stake_lamports,
                activation_epoch: clock.epoch,
                deactivation_epoch: u64::MAX,
                ..Delegation::default()
            },
            credits_observed: vote_state_credits,
        }
    );

    // signed but faked vote account
    transaction_accounts[1] = (vote_address_2, vote_account_2);
    transaction_accounts[1]
        .1
        .set_owner(solana_sdk::pubkey::new_rand());
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::IncorrectProgramId),
    );

    // verify that non-stakes fail delegate()
    let stake_state = StakeStateV2::RewardsPool;
    stake_account.set_state(&stake_state).unwrap();
    transaction_accounts[0] = (stake_address, stake_account);
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::IncorrectProgramId),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_redelegate_consider_balance_changes(mollusk: Mollusk) {
    let mut clock = Clock::default();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let initial_lamports = 4242424242;
    let stake_lamports = rent_exempt_reserve + initial_lamports;
    let recipient_address = solana_sdk::pubkey::new_rand();
    let authority_address = solana_sdk::pubkey::new_rand();
    let vote_address = solana_sdk::pubkey::new_rand();
    let vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Initialized(Meta {
            rent_exempt_reserve,
            ..Meta::auto(&authority_address)
        }),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (vote_address, vote_account),
        (
            recipient_address,
            AccountSharedData::new(1, 0, &system_program::id()),
        ),
        (authority_address, AccountSharedData::default()),
        (clock::id(), create_account_shared_data_for_test(&clock)),
        (stake_history::id(), create_empty_stake_history_for_test()),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    #[allow(deprecated)]
    let delegate_instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: vote_address,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_config::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authority_address,
            is_signer: true,
            is_writable: false,
        },
    ];
    let deactivate_instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authority_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        delegate_instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    clock.epoch += 1;
    transaction_accounts[2] = (clock::id(), create_account_shared_data_for_test(&clock));
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        deactivate_instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // Once deactivated, we withdraw stake to new account
    clock.epoch += 1;
    transaction_accounts[2] = (clock::id(), create_account_shared_data_for_test(&clock));
    let withdraw_lamports = initial_lamports / 2;
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(withdraw_lamports)).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    let expected_balance = rent_exempt_reserve + initial_lamports - withdraw_lamports;
    assert_eq!(accounts[0].lamports(), expected_balance);
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    clock.epoch += 1;
    transaction_accounts[2] = (clock::id(), create_account_shared_data_for_test(&clock));
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        delegate_instruction_accounts.clone(),
        Ok(()),
    );
    assert_eq!(
        stake_from(&accounts[0]).unwrap().delegation.stake,
        accounts[0].lamports() - rent_exempt_reserve,
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    clock.epoch += 1;
    transaction_accounts[2] = (clock::id(), create_account_shared_data_for_test(&clock));
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        deactivate_instruction_accounts,
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // Out of band deposit
    transaction_accounts[0]
        .1
        .checked_add_lamports(withdraw_lamports)
        .unwrap();

    clock.epoch += 1;
    transaction_accounts[2] = (clock::id(), create_account_shared_data_for_test(&clock));
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts,
        delegate_instruction_accounts,
        Ok(()),
    );
    assert_eq!(
        stake_from(&accounts[0]).unwrap().delegation.stake,
        accounts[0].lamports() - rent_exempt_reserve,
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split(mollusk: Mollusk) {
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let stake_address = solana_sdk::pubkey::new_rand();
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = minimum_delegation * 2;
    let split_to_address = solana_sdk::pubkey::new_rand();
    let split_to_account = AccountSharedData::new_data_with_space(
        0,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let mut transaction_accounts = vec![
        (stake_address, AccountSharedData::default()),
        (split_to_address, split_to_account.clone()),
        (
            rent::id(),
            create_account_shared_data_for_test(&Rent {
                lamports_per_byte_year: 0,
                ..Rent::default()
            }),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&stake_history),
        ),
        (clock::id(), create_account_shared_data_for_test(&clock)),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    for state in [
        StakeStateV2::Initialized(Meta::auto(&stake_address)),
        just_stake(Meta::auto(&stake_address), stake_lamports),
    ] {
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let expected_active_stake = get_active_stake_for_tests(
            &[stake_account.clone(), split_to_account.clone()],
            &clock,
            &stake_history,
        );
        transaction_accounts[0] = (stake_address, stake_account);

        // should fail, split more than available
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );

        // should pass
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        // no lamport leakage
        assert_eq!(
            accounts[0].lamports() + accounts[1].lamports(),
            stake_lamports
        );

        // no deactivated stake
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &stake_history)
        );

        assert_eq!(from(&accounts[0]).unwrap(), from(&accounts[1]).unwrap());
        match state {
            StakeStateV2::Initialized(_meta) => {
                assert_eq!(from(&accounts[0]).unwrap(), state);
            }
            StakeStateV2::Stake(_meta, _stake, _) => {
                let stake_0 = from(&accounts[0]).unwrap().stake();
                assert_eq!(stake_0.unwrap().delegation.stake, stake_lamports / 2);
            }
            _ => unreachable!(),
        }
    }

    // should fail, fake owner of destination
    let split_to_account = AccountSharedData::new_data_with_space(
        0,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &solana_sdk::pubkey::new_rand(),
    )
    .unwrap();
    transaction_accounts[1] = (split_to_address, split_to_account);
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(if mollusk.is_bpf() {
            ProgramError::InvalidAccountOwner
        } else {
            ProgramError::IncorrectProgramId
        }),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_withdraw_stake(mollusk: Mollusk) {
    let recipient_address = solana_sdk::pubkey::new_rand();
    let authority_address = solana_sdk::pubkey::new_rand();
    let custodian_address = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = minimum_delegation;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let vote_address = solana_sdk::pubkey::new_rand();
    let mut vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    vote_account
        .set_state(&VoteStateVersions::new_current(VoteState::default()))
        .unwrap();
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (vote_address, vote_account),
        (recipient_address, AccountSharedData::default()),
        (
            authority_address,
            AccountSharedData::new(42, 0, &system_program::id()),
        ),
        (custodian_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            rent::id(),
            create_account_shared_data_for_test(&Rent::free()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: recipient_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    // should fail, no signer
    instruction_accounts[4].is_signer = false;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );
    instruction_accounts[4].is_signer = true;

    // should pass, signed keyed account and uninitialized
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    assert_eq!(accounts[0].lamports(), 0);
    assert_eq!(from(&accounts[0]).unwrap(), StakeStateV2::Uninitialized);

    // initialize stake
    let lockup = Lockup {
        unix_timestamp: 0,
        epoch: 0,
        custodian: custodian_address,
    };
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Initialize(
            Authorized::auto(&stake_address),
            lockup,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rent::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // should fail, signed keyed account and locked up, more than available
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports + 1)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InsufficientFunds),
    );

    // Stake some lamports (available lamports for withdrawals will reduce to zero)
    #[allow(deprecated)]
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // simulate rewards
    transaction_accounts[0].1.checked_add_lamports(10).unwrap();

    // withdrawal before deactivate works for rewards amount
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(10)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    // withdrawal of rewards fails if not in excess of stake
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(11)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InsufficientFunds),
    );

    // deactivate the stake before withdrawal
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // simulate time passing
    let clock = Clock {
        epoch: 100,
        ..Clock::default()
    };
    transaction_accounts[5] = (clock::id(), create_account_shared_data_for_test(&clock));

    // Try to withdraw more than what's available
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports + 11)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InsufficientFunds),
    );

    // Try to withdraw all lamports
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports + 10)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    assert_eq!(accounts[0].lamports(), 0);
    assert_eq!(from(&accounts[0]).unwrap(), StakeStateV2::Uninitialized);

    // overflow
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_account = AccountSharedData::new_data_with_space(
        1_000_000_000,
        &StakeStateV2::Initialized(Meta {
            rent_exempt_reserve,
            authorized: Authorized {
                staker: authority_address,
                withdrawer: authority_address,
            },
            lockup: Lockup::default(),
        }),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    transaction_accounts[0] = (stake_address, stake_account.clone());
    transaction_accounts[2] = (recipient_address, stake_account);
    instruction_accounts[4].pubkey = authority_address;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(u64::MAX - 10)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InsufficientFunds),
    );

    // should fail, invalid state
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::RewardsPool,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    transaction_accounts[0] = (stake_address, stake_account);
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::InvalidAccountData),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_withdraw_stake_before_warmup(mollusk: Mollusk) {
    let recipient_address = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = minimum_delegation;
    let total_lamports = stake_lamports + 33;
    let stake_account = AccountSharedData::new_data_with_space(
        total_lamports,
        &StakeStateV2::Initialized(Meta::auto(&stake_address)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let vote_address = solana_sdk::pubkey::new_rand();
    let mut vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    vote_account
        .set_state(&VoteStateVersions::new_current(VoteState::default()))
        .unwrap();
    let mut clock = Clock {
        epoch: 16,
        ..Clock::default()
    };
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (vote_address, vote_account),
        (recipient_address, AccountSharedData::default()),
        (clock::id(), create_account_shared_data_for_test(&clock)),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: recipient_address,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    // Stake some lamports (available lamports for withdrawals will reduce to zero)
    #[allow(deprecated)]
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // Try to withdraw stake
    let stake_history = create_stake_history_from_delegations(
        None,
        0..clock.epoch,
        &[stake_from(&accounts[0]).unwrap().delegation],
        None,
    );
    transaction_accounts[4] = (
        stake_history::id(),
        create_account_shared_data_for_test(&stake_history),
    );
    clock.epoch = 0;
    transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(
            total_lamports - stake_lamports + 1,
        ))
        .unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::InsufficientFunds),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_withdraw_lockup(mollusk: Mollusk) {
    let recipient_address = solana_sdk::pubkey::new_rand();
    let custodian_address = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let total_lamports = 100;
    let mut meta = Meta {
        lockup: Lockup {
            unix_timestamp: 0,
            epoch: 1,
            custodian: custodian_address,
        },
        ..Meta::auto(&stake_address)
    };
    let stake_account = AccountSharedData::new_data_with_space(
        total_lamports,
        &StakeStateV2::Initialized(meta),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let mut clock = Clock::default();
    let mut transaction_accounts = vec![
        (stake_address, stake_account.clone()),
        (recipient_address, AccountSharedData::default()),
        (custodian_address, AccountSharedData::default()),
        (clock::id(), create_account_shared_data_for_test(&clock)),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: recipient_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    // should fail, lockup is still in force
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(StakeError::LockupInForce.into()),
    );

    // should pass
    instruction_accounts.push(AccountMeta {
        pubkey: custodian_address,
        is_signer: true,
        is_writable: false,
    });
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    assert_eq!(from(&accounts[0]).unwrap(), StakeStateV2::Uninitialized);

    // should pass, custodian is the same as the withdraw authority
    instruction_accounts[5].pubkey = stake_address;
    meta.lockup.custodian = stake_address;
    let stake_account_self_as_custodian = AccountSharedData::new_data_with_space(
        total_lamports,
        &StakeStateV2::Initialized(meta),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    transaction_accounts[0] = (stake_address, stake_account_self_as_custodian);
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    assert_eq!(from(&accounts[0]).unwrap(), StakeStateV2::Uninitialized);
    transaction_accounts[0] = (stake_address, stake_account);

    // should pass, lockup has expired
    instruction_accounts.pop();
    clock.epoch += 1;
    transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Ok(()),
    );
    assert_eq!(from(&accounts[0]).unwrap(), StakeStateV2::Uninitialized);
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_withdraw_rent_exempt(mollusk: Mollusk) {
    let recipient_address = solana_sdk::pubkey::new_rand();
    let custodian_address = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = 7 * minimum_delegation;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports + rent_exempt_reserve,
        &StakeStateV2::Initialized(Meta {
            rent_exempt_reserve,
            ..Meta::auto(&stake_address)
        }),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let transaction_accounts = vec![
        (stake_address, stake_account),
        (recipient_address, AccountSharedData::default()),
        (custodian_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: recipient_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    // should pass, withdrawing initialized account down to minimum balance
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    // should fail, withdrawal that would leave less than rent-exempt reserve
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(stake_lamports + 1)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InsufficientFunds),
    );

    // should pass, withdrawal of complete account
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(
            stake_lamports + rent_exempt_reserve,
        ))
        .unwrap(),
        transaction_accounts,
        instruction_accounts,
        Ok(()),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_deactivate(mollusk: Mollusk) {
    let stake_address = solana_sdk::pubkey::new_rand();
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = minimum_delegation;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Initialized(Meta::auto(&stake_address)),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let vote_address = solana_sdk::pubkey::new_rand();
    let mut vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    vote_account
        .set_state(&VoteStateVersions::new_current(VoteState::default()))
        .unwrap();
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (vote_address, vote_account),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    // should fail, not signed
    instruction_accounts[0].is_signer = false;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InvalidAccountData),
    );
    instruction_accounts[0].is_signer = true;

    // should fail, not staked yet
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InvalidAccountData),
    );

    // Staking
    #[allow(deprecated)]
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // should pass
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // should fail, only works once
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(StakeError::AlreadyDeactivated.into()),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_set_lockup(mollusk: Mollusk) {
    let custodian_address = solana_sdk::pubkey::new_rand();
    let authorized_address = solana_sdk::pubkey::new_rand();
    let stake_address = solana_sdk::pubkey::new_rand();
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = minimum_delegation;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let vote_address = solana_sdk::pubkey::new_rand();
    let mut vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    vote_account
        .set_state(&VoteStateVersions::new_current(VoteState::default()))
        .unwrap();
    let instruction_data = serialize(&StakeInstruction::SetLockup(LockupArgs {
        unix_timestamp: Some(1),
        epoch: Some(1),
        custodian: Some(custodian_address),
    }))
    .unwrap();
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (vote_address, vote_account),
        (authorized_address, AccountSharedData::default()),
        (custodian_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            rent::id(),
            create_account_shared_data_for_test(&Rent::free()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: custodian_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    // should fail, wrong state
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::InvalidAccountData),
    );

    // initialize stake
    let lockup = Lockup {
        unix_timestamp: 1,
        epoch: 1,
        custodian: custodian_address,
    };
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Initialize(
            Authorized::auto(&stake_address),
            lockup,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rent::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // should fail, not signed
    instruction_accounts[2].is_signer = false;
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );
    instruction_accounts[2].is_signer = true;

    // should pass
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    // Staking
    #[allow(deprecated)]
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // should fail, not signed
    instruction_accounts[2].is_signer = false;
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );
    instruction_accounts[2].is_signer = true;

    // should pass
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    // Lockup in force
    let instruction_data = serialize(&StakeInstruction::SetLockup(LockupArgs {
        unix_timestamp: Some(2),
        epoch: None,
        custodian: None,
    }))
    .unwrap();

    // should fail, authorized withdrawer cannot change it
    instruction_accounts[0].is_signer = true;
    instruction_accounts[2].is_signer = false;
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );
    instruction_accounts[0].is_signer = false;
    instruction_accounts[2].is_signer = true;

    // should pass, custodian can change it
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    // Lockup expired
    let clock = Clock {
        unix_timestamp: i64::MAX,
        epoch: Epoch::MAX,
        ..Clock::default()
    };
    transaction_accounts[4] = (clock::id(), create_account_shared_data_for_test(&clock));

    // should fail, custodian cannot change it
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Err(ProgramError::MissingRequiredSignature),
    );

    // should pass, authorized withdrawer can change it
    instruction_accounts[0].is_signer = true;
    instruction_accounts[2].is_signer = false;
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    // Change authorized withdrawer
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Authorize(
            authorized_address,
            StakeAuthorize::Withdrawer,
        ))
        .unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authorized_address,
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    // should fail, previous authorized withdrawer cannot change the lockup anymore
    process_instruction(
        &mollusk,
        &instruction_data,
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::MissingRequiredSignature),
    );
}

/// Ensure that `initialize()` respects the minimum balance requirements
/// - Assert 1: accounts with a balance equal-to the rent exemption initialize OK
/// - Assert 2: accounts with a balance less-than the rent exemption do not initialize
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_initialize_minimum_balance(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_address = solana_sdk::pubkey::new_rand();
    let instruction_data = serialize(&StakeInstruction::Initialize(
        Authorized::auto(&stake_address),
        Lockup::default(),
    ))
    .unwrap();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: rent::id(),
            is_signer: false,
            is_writable: false,
        },
    ];
    for (lamports, expected_result) in [
        (rent_exempt_reserve, Ok(())),
        (
            rent_exempt_reserve - 1,
            Err(ProgramError::InsufficientFunds),
        ),
    ] {
        let stake_account = AccountSharedData::new(lamports, StakeStateV2::size_of(), &id());
        process_instruction(
            &mollusk,
            &instruction_data,
            vec![
                (stake_address, stake_account),
                (rent::id(), create_account_shared_data_for_test(&rent)),
            ],
            instruction_accounts.clone(),
            expected_result,
        );
    }
}

/// Ensure that `delegate()` respects the minimum delegation requirements
/// - Assert 1: delegating an amount equal-to the minimum succeeds
/// - Assert 2: delegating an amount less-than the minimum fails
/// Also test both asserts above over both StakeStateV2::{Initialized and Stake}, since the logic
/// is slightly different for the variants.
///
/// NOTE: Even though new stake accounts must have a minimum balance that is at least
/// the minimum delegation (plus rent exempt reserve), the old behavior allowed
/// withdrawing below the minimum delegation, then re-delegating successfully (see
/// `test_behavior_withdrawal_then_redelegate_with_less_than_minimum_stake_delegation()` for
/// more information.)
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_delegate_minimum_stake_delegation(mollusk: Mollusk) {
    let minimum_delegation = crate::get_minimum_delegation();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        rent_exempt_reserve,
        ..Meta::auto(&stake_address)
    };
    let vote_address = solana_sdk::pubkey::new_rand();
    let vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    #[allow(deprecated)]
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: vote_address,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_config::id(),
            is_signer: false,
            is_writable: false,
        },
    ];
    for (stake_delegation, expected_result) in &[
        (minimum_delegation, Ok(())),
        (
            minimum_delegation - 1,
            Err(StakeError::InsufficientDelegation),
        ),
    ] {
        for stake_state in &[
            StakeStateV2::Initialized(meta),
            just_stake(meta, *stake_delegation),
        ] {
            let stake_account = AccountSharedData::new_data_with_space(
                stake_delegation + rent_exempt_reserve,
                stake_state,
                StakeStateV2::size_of(),
                &id(),
            )
            .unwrap();
            #[allow(deprecated)]
            process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                vec![
                    (stake_address, stake_account),
                    (vote_address, vote_account.clone()),
                    (
                        clock::id(),
                        create_account_shared_data_for_test(&Clock::default()),
                    ),
                    (
                        stake_history::id(),
                        create_account_shared_data_for_test(&StakeHistory::default()),
                    ),
                    (
                        stake_config::id(),
                        config::create_account(0, &stake_config::Config::default()),
                    ),
                    (
                        epoch_schedule::id(),
                        create_account_shared_data_for_test(&EpochSchedule::default()),
                    ),
                ],
                instruction_accounts.clone(),
                expected_result.clone().map_err(|e| e.into()),
            );
        }
    }
}

/// Ensure that `split()` respects the minimum delegation requirements.  This applies to
/// both the source and destination acounts.  Thus, we have four permutations possible based on
/// if each account's post-split delegation is equal-to (EQ) or less-than (LT) the minimum:
///
///  source | dest | result
/// --------+------+--------
///  EQ     | EQ   | Ok
///  EQ     | LT   | Err
///  LT     | EQ   | Err
///  LT     | LT   | Err
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_minimum_stake_delegation(mollusk: Mollusk) {
    let minimum_delegation = crate::get_minimum_delegation();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let source_address = Pubkey::new_unique();
    let source_meta = Meta {
        rent_exempt_reserve,
        ..Meta::auto(&source_address)
    };
    let dest_address = Pubkey::new_unique();
    let dest_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: source_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: dest_address,
            is_signer: false,
            is_writable: true,
        },
    ];
    for (source_delegation, split_amount, expected_result) in [
        (minimum_delegation * 2, minimum_delegation, Ok(())),
        (
            minimum_delegation * 2,
            minimum_delegation - 1,
            Err(ProgramError::InsufficientFunds),
        ),
        (
            (minimum_delegation * 2) - 1,
            minimum_delegation,
            Err(ProgramError::InsufficientFunds),
        ),
        (
            (minimum_delegation - 1) * 2,
            minimum_delegation - 1,
            Err(ProgramError::InsufficientFunds),
        ),
    ] {
        let source_account = AccountSharedData::new_data_with_space(
            source_delegation + rent_exempt_reserve,
            &just_stake(source_meta, source_delegation),
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let expected_active_stake = get_active_stake_for_tests(
            &[source_account.clone(), dest_account.clone()],
            &clock,
            &stake_history,
        );
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
            vec![
                (source_address, source_account),
                (dest_address, dest_account.clone()),
                (rent::id(), create_account_shared_data_for_test(&rent)),
                (
                    stake_history::id(),
                    create_account_shared_data_for_test(&stake_history),
                ),
                (clock::id(), create_account_shared_data_for_test(&clock)),
                (
                    epoch_schedule::id(),
                    create_account_shared_data_for_test(&EpochSchedule::default()),
                ),
            ],
            instruction_accounts.clone(),
            expected_result.clone(),
        );
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &stake_history)
        );
    }
}

/// Ensure that splitting the full amount from an account respects the minimum delegation
/// requirements.  This ensures that we are future-proofing/testing any raises to the minimum
/// delegation.
/// - Assert 1: splitting the full amount from an account that has at least the minimum
///             delegation is OK
/// - Assert 2: splitting the full amount from an account that has less than the minimum
///             delegation is not OK
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_full_amount_minimum_stake_delegation(mollusk: Mollusk) {
    let minimum_delegation = crate::get_minimum_delegation();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let source_address = Pubkey::new_unique();
    let source_meta = Meta {
        rent_exempt_reserve,
        ..Meta::auto(&source_address)
    };
    let dest_address = Pubkey::new_unique();
    let dest_account = AccountSharedData::new_data_with_space(
        0,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: source_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: dest_address,
            is_signer: false,
            is_writable: true,
        },
    ];
    for (reserve, expected_result) in [
        (rent_exempt_reserve, Ok(())),
        (
            rent_exempt_reserve - 1,
            Err(ProgramError::InsufficientFunds),
        ),
    ] {
        for (stake_delegation, source_stake_state) in &[
            (0, StakeStateV2::Initialized(source_meta)),
            (
                minimum_delegation,
                just_stake(source_meta, minimum_delegation),
            ),
        ] {
            let source_account = AccountSharedData::new_data_with_space(
                stake_delegation + reserve,
                source_stake_state,
                StakeStateV2::size_of(),
                &id(),
            )
            .unwrap();
            let expected_active_stake = get_active_stake_for_tests(
                &[source_account.clone(), dest_account.clone()],
                &clock,
                &stake_history,
            );
            let accounts = process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::Split(source_account.lamports())).unwrap(),
                vec![
                    (source_address, source_account),
                    (dest_address, dest_account.clone()),
                    (rent::id(), create_account_shared_data_for_test(&rent)),
                    (
                        stake_history::id(),
                        create_account_shared_data_for_test(&stake_history),
                    ),
                    (clock::id(), create_account_shared_data_for_test(&clock)),
                    (
                        epoch_schedule::id(),
                        create_account_shared_data_for_test(&EpochSchedule::default()),
                    ),
                ],
                instruction_accounts.clone(),
                expected_result.clone(),
            );
            assert_eq!(
                expected_active_stake,
                get_active_stake_for_tests(&accounts[0..2], &clock, &stake_history)
            );
        }
    }
}

/// Ensure that `split()` correctly handles prefunded destination accounts from
/// initialized stakes.  When a destination account already has funds, ensure
/// the minimum split amount reduces accordingly.
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_initialized_split_destination_minimum_balance(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let source_address = Pubkey::new_unique();
    let destination_address = Pubkey::new_unique();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: source_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: destination_address,
            is_signer: false,
            is_writable: true,
        },
    ];
    for (destination_starting_balance, split_amount, expected_result) in [
        // split amount must be non zero
        (rent_exempt_reserve, 0, Err(ProgramError::InsufficientFunds)),
        // any split amount is OK when destination account is already fully funded
        (rent_exempt_reserve, 1, Ok(())),
        // if destination is only short by 1 lamport, then split amount can be 1 lamport
        (rent_exempt_reserve - 1, 1, Ok(())),
        // destination short by 2 lamports, then 1 isn't enough (non-zero split amount)
        (
            rent_exempt_reserve - 2,
            1,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination has smallest non-zero balance, so can split the minimum balance
        // requirements minus what destination already has
        (1, rent_exempt_reserve - 1, Ok(())),
        // destination has smallest non-zero balance, but cannot split less than the minimum
        // balance requirements minus what destination already has
        (
            1,
            rent_exempt_reserve - 2,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination has zero lamports, so split must be at least rent exempt reserve
        (0, rent_exempt_reserve, Ok(())),
        // destination has zero lamports, but split amount is less than rent exempt reserve
        (
            0,
            rent_exempt_reserve - 1,
            Err(ProgramError::InsufficientFunds),
        ),
    ] {
        // Set the source's starting balance to something large to ensure its post-split
        // balance meets all the requirements
        let source_balance = rent_exempt_reserve + split_amount;
        let source_meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&source_address)
        };
        let source_account = AccountSharedData::new_data_with_space(
            source_balance,
            &StakeStateV2::Initialized(source_meta),
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let destination_account = AccountSharedData::new_data_with_space(
            destination_starting_balance,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();

        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
            vec![
                (source_address, source_account),
                (destination_address, destination_account),
                (rent::id(), create_account_shared_data_for_test(&rent)),
            ],
            instruction_accounts.clone(),
            expected_result.clone(),
        );
    }
}

/// Ensure that `split()` correctly handles prefunded destination accounts from staked stakes.
/// When a destination account already has funds, ensure the minimum split amount reduces
/// accordingly.
#[test_case(mollusk_native(), &[Ok(()), Ok(())]; "native_stake")]
#[test_case(mollusk_bpf(), &[Ok(()), Ok(())]; "bpf_stake")]
// NOTE it is not presently possible to test 1sol minimum delegation
// #[test_case(feature_set_all_enabled(), &[Err(StakeError::InsufficientDelegation.into()), Err(StakeError::InsufficientDelegation.into())]; "all_enabled")]
fn test_staked_split_destination_minimum_balance(
    mollusk: Mollusk,
    expected_results: &[Result<(), ProgramError>],
) {
    let minimum_delegation = crate::get_minimum_delegation();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let source_address = Pubkey::new_unique();
    let destination_address = Pubkey::new_unique();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: source_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: destination_address,
            is_signer: false,
            is_writable: true,
        },
    ];
    for (destination_starting_balance, split_amount, expected_result) in [
        // split amount must be non zero
        (
            rent_exempt_reserve + minimum_delegation,
            0,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination is fully funded:
        // - old behavior: any split amount is OK
        // - new behavior: split amount must be at least the minimum delegation
        (
            rent_exempt_reserve + minimum_delegation,
            1,
            expected_results[0].clone(),
        ),
        // if destination is only short by 1 lamport, then...
        // - old behavior: split amount can be 1 lamport
        // - new behavior: split amount must be at least the minimum delegation
        (
            rent_exempt_reserve + minimum_delegation - 1,
            1,
            expected_results[1].clone(),
        ),
        // destination short by 2 lamports, so 1 isn't enough (non-zero split amount)
        (
            rent_exempt_reserve + minimum_delegation - 2,
            1,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination is rent exempt, so split enough for minimum delegation
        (rent_exempt_reserve, minimum_delegation, Ok(())),
        // destination is rent exempt, but split amount less than minimum delegation
        (
            rent_exempt_reserve,
            minimum_delegation.saturating_sub(1), // when minimum is 0, this blows up!
            Err(ProgramError::InsufficientFunds),
        ),
        // destination is not rent exempt, so any split amount fails, including enough for rent
        // and minimum delegation
        (
            rent_exempt_reserve - 1,
            minimum_delegation + 1,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination is not rent exempt, but split amount only for minimum delegation
        (
            rent_exempt_reserve - 1,
            minimum_delegation,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination is not rent exempt, so any split amount fails, including case where
        // destination has smallest non-zero balance
        (
            1,
            rent_exempt_reserve + minimum_delegation - 1,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination has smallest non-zero balance, but cannot split less than the minimum
        // balance requirements minus what destination already has
        (
            1,
            rent_exempt_reserve + minimum_delegation - 2,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination has zero lamports, so any split amount fails, including at least rent
        // exempt reserve plus minimum delegation
        (
            0,
            rent_exempt_reserve + minimum_delegation,
            Err(ProgramError::InsufficientFunds),
        ),
        // destination has zero lamports, but split amount is less than rent exempt reserve
        // plus minimum delegation
        (
            0,
            rent_exempt_reserve + minimum_delegation - 1,
            Err(ProgramError::InsufficientFunds),
        ),
    ] {
        // Set the source's starting balance to something large to ensure its post-split
        // balance meets all the requirements
        let source_balance = rent_exempt_reserve + minimum_delegation + split_amount;
        let source_meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&source_address)
        };
        let source_stake_delegation = source_balance - rent_exempt_reserve;
        let source_account = AccountSharedData::new_data_with_space(
            source_balance,
            &just_stake(source_meta, source_stake_delegation),
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let destination_account = AccountSharedData::new_data_with_space(
            destination_starting_balance,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let expected_active_stake = get_active_stake_for_tests(
            &[source_account.clone(), destination_account.clone()],
            &clock,
            &StakeHistory::default(),
        );
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
            vec![
                (source_address, source_account.clone()),
                (destination_address, destination_account),
                (rent::id(), create_account_shared_data_for_test(&rent)),
                (stake_history::id(), create_empty_stake_history_for_test()),
                (clock::id(), create_account_shared_data_for_test(&clock)),
                (
                    epoch_schedule::id(),
                    create_account_shared_data_for_test(&EpochSchedule::default()),
                ),
            ],
            instruction_accounts.clone(),
            expected_result.clone(),
        );
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &StakeHistory::default())
        );
        // For the expected OK cases, when the source's StakeStateV2 is Stake, then the
        // destination's StakeStateV2 *must* also end up as Stake as well.  Additionally,
        // check to ensure the destination's delegation amount is correct.  If the
        // destination is already rent exempt, then the destination's stake delegation
        // *must* equal the split amount. Otherwise, the split amount must first be used to
        // make the destination rent exempt, and then the leftover lamports are delegated.
        if expected_result.is_ok() {
            assert_matches!(accounts[0].state().unwrap(), StakeStateV2::Stake(_, _, _));
            if let StakeStateV2::Stake(_, destination_stake, _) = accounts[1].state().unwrap() {
                let destination_initial_rent_deficit =
                    rent_exempt_reserve.saturating_sub(destination_starting_balance);
                let expected_destination_stake_delegation =
                    split_amount - destination_initial_rent_deficit;
                assert_eq!(
                    expected_destination_stake_delegation,
                    destination_stake.delegation.stake
                );
                assert!(destination_stake.delegation.stake >= minimum_delegation,);
            } else {
                panic!("destination state must be StakeStake::Stake after successful split when source is also StakeStateV2::Stake!");
            }
        }
    }
}

/// Ensure that `withdraw()` respects the minimum delegation requirements
/// - Assert 1: withdrawing so remaining stake is equal-to the minimum is OK
/// - Assert 2: withdrawing so remaining stake is less-than the minimum is not OK
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_withdraw_minimum_stake_delegation(mollusk: Mollusk) {
    let minimum_delegation = crate::get_minimum_delegation();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        rent_exempt_reserve,
        ..Meta::auto(&stake_address)
    };
    let recipient_address = solana_sdk::pubkey::new_rand();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: recipient_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: false,
        },
    ];
    let starting_stake_delegation = minimum_delegation;
    for (ending_stake_delegation, expected_result) in [
        (minimum_delegation, Ok(())),
        (minimum_delegation - 1, Err(ProgramError::InsufficientFunds)),
    ] {
        for (stake_delegation, stake_state) in &[
            (0, StakeStateV2::Initialized(meta)),
            (minimum_delegation, just_stake(meta, minimum_delegation)),
        ] {
            let rewards_balance = 123;
            let stake_account = AccountSharedData::new_data_with_space(
                stake_delegation + rent_exempt_reserve + rewards_balance,
                stake_state,
                StakeStateV2::size_of(),
                &id(),
            )
            .unwrap();
            let withdraw_amount =
                (starting_stake_delegation + rewards_balance) - ending_stake_delegation;
            #[allow(deprecated)]
            process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::Withdraw(withdraw_amount)).unwrap(),
                vec![
                    (stake_address, stake_account),
                    (
                        recipient_address,
                        AccountSharedData::new(rent_exempt_reserve, 0, &system_program::id()),
                    ),
                    (
                        clock::id(),
                        create_account_shared_data_for_test(&Clock::default()),
                    ),
                    (
                        rent::id(),
                        create_account_shared_data_for_test(&Rent::free()),
                    ),
                    (
                        stake_history::id(),
                        create_account_shared_data_for_test(&StakeHistory::default()),
                    ),
                    (
                        stake_config::id(),
                        config::create_account(0, &stake_config::Config::default()),
                    ),
                    (
                        epoch_schedule::id(),
                        create_account_shared_data_for_test(&EpochSchedule::default()),
                    ),
                ],
                instruction_accounts.clone(),
                expected_result.clone(),
            );
        }
    }
}

/// The stake program's old behavior allowed delegations below the minimum stake delegation
/// (see also `test_delegate_minimum_stake_delegation()`).  This was not the desired behavior,
/// and has been fixed in the new behavior.  This test ensures the behavior is not changed
/// inadvertently.
///
/// This test:
/// 1. Initialises a stake account (with sufficient balance for both rent and minimum delegation)
/// 2. Delegates the minimum amount
/// 3. Deactives the delegation
/// 4. Withdraws from the account such that the ending balance is *below* rent + minimum delegation
/// 5. Re-delegates, now with less than the minimum delegation, but it still succeeds
#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_behavior_withdrawal_then_redelegate_with_less_than_minimum_stake_delegation(
    mollusk: Mollusk,
) {
    let minimum_delegation = crate::get_minimum_delegation();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_account = AccountSharedData::new(
        rent_exempt_reserve + minimum_delegation,
        StakeStateV2::size_of(),
        &id(),
    );
    let vote_address = solana_sdk::pubkey::new_rand();
    let vote_account =
        vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
    let recipient_address = solana_sdk::pubkey::new_rand();
    let mut clock = Clock::default();
    #[allow(deprecated)]
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (vote_address, vote_account),
        (
            recipient_address,
            AccountSharedData::new(rent_exempt_reserve, 0, &system_program::id()),
        ),
        (clock::id(), create_account_shared_data_for_test(&clock)),
        (stake_history::id(), create_empty_stake_history_for_test()),
        (
            stake_config::id(),
            config::create_account(0, &stake_config::Config::default()),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
        (rent::id(), create_account_shared_data_for_test(&rent)),
    ];
    #[allow(deprecated)]
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: vote_address,
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_config::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Initialize(
            Authorized::auto(&stake_address),
            Lockup::default(),
        ))
        .unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rent::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());
    transaction_accounts[1] = (vote_address, accounts[1].clone());

    clock.epoch += 1;
    transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Deactivate).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    clock.epoch += 1;
    transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));
    let withdraw_amount = accounts[0].lamports() - (rent_exempt_reserve + minimum_delegation - 1);
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Withdraw(withdraw_amount)).unwrap(),
        transaction_accounts.clone(),
        vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
        ],
        Ok(()),
    );
    transaction_accounts[0] = (stake_address, accounts[0].clone());

    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::DelegateStake).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(StakeError::InsufficientDelegation.into()),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_source_uninitialized(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = (rent_exempt_reserve + minimum_delegation) * 2;
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let split_to_account = AccountSharedData::new_data_with_space(
        0,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let transaction_accounts = vec![
        (stake_address, stake_account),
        (split_to_address, split_to_account),
    ];
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    // splitting an uninitialized account where the destination is the same as the source
    {
        // splitting should work when...
        // - when split amount is the full balance
        // - when split amount is zero
        // - when split amount is non-zero and less than the full balance
        //
        // and splitting should fail when the split amount is greater than the balance
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(0)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );
    }

    // this should work
    instruction_accounts[1].pubkey = split_to_address;
    let accounts = process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );
    assert_eq!(accounts[0].lamports(), accounts[1].lamports());

    // no signers should fail
    instruction_accounts[0].is_signer = false;
    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::MissingRequiredSignature),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_split_not_uninitialized(mollusk: Mollusk) {
    let stake_lamports = 42;
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &just_stake(Meta::auto(&stake_address), stake_lamports),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    for split_to_state in &[
        StakeStateV2::Initialized(Meta::default()),
        StakeStateV2::Stake(Meta::default(), Stake::default(), StakeFlags::default()),
        StakeStateV2::RewardsPool,
    ] {
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            split_to_state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            vec![
                (stake_address, stake_account.clone()),
                (split_to_address, split_to_account),
            ],
            instruction_accounts.clone(),
            Err(ProgramError::InvalidAccountData),
        );
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_more_than_staked(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = (rent_exempt_reserve + minimum_delegation) * 2;
    let stake_address = solana_sdk::pubkey::new_rand();
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &just_stake(
            Meta {
                rent_exempt_reserve,
                ..Meta::auto(&stake_address)
            },
            stake_lamports / 2 - 1,
        ),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let split_to_account = AccountSharedData::new_data_with_space(
        rent_exempt_reserve,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let transaction_accounts = vec![
        (stake_address, stake_account),
        (split_to_address, split_to_account),
        (rent::id(), create_account_shared_data_for_test(&rent)),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&stake_history),
        ),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock {
                epoch: current_epoch,
                ..Clock::default()
            }),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(StakeError::InsufficientDelegation.into()),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_with_rent(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_address = solana_sdk::pubkey::new_rand();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let split_to_account = AccountSharedData::new_data_with_space(
        0,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];
    let meta = Meta {
        authorized: Authorized::auto(&stake_address),
        rent_exempt_reserve,
        ..Meta::default()
    };

    // test splitting both an Initialized stake and a Staked stake
    for (minimum_balance, state) in &[
        (rent_exempt_reserve, StakeStateV2::Initialized(meta)),
        (
            rent_exempt_reserve + minimum_delegation,
            just_stake(meta, minimum_delegation * 2 + rent_exempt_reserve),
        ),
    ] {
        let stake_lamports = minimum_balance * 2;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let expected_active_stake = get_active_stake_for_tests(
            &[stake_account.clone(), split_to_account.clone()],
            &clock,
            &stake_history,
        );
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (split_to_address, split_to_account.clone()),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (
                stake_history::id(),
                create_account_shared_data_for_test(&stake_history),
            ),
            (clock::id(), create_account_shared_data_for_test(&clock)),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ];

        // not enough to make a non-zero stake account
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(minimum_balance - 1)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );

        // doesn't leave enough for initial stake to be non-zero
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(
                stake_lamports - minimum_balance + 1,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );

        // split account already has enough lamports
        transaction_accounts[1].1.set_lamports(*minimum_balance);
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports - minimum_balance)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &stake_history)
        );

        // verify no stake leakage in the case of a stake
        if let StakeStateV2::Stake(meta, stake, stake_flags) = state {
            assert_eq!(
                accounts[1].state(),
                Ok(StakeStateV2::Stake(
                    *meta,
                    Stake {
                        delegation: Delegation {
                            stake: stake_lamports - minimum_balance,
                            ..stake.delegation
                        },
                        ..*stake
                    },
                    *stake_flags,
                ))
            );
            assert_eq!(accounts[0].lamports(), *minimum_balance,);
            assert_eq!(accounts[1].lamports(), stake_lamports,);
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_to_account_with_rent_exempt_reserve(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = (rent_exempt_reserve + minimum_delegation) * 2;
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        authorized: Authorized::auto(&stake_address),
        rent_exempt_reserve,
        ..Meta::default()
    };
    let state = just_stake(meta, stake_lamports - rent_exempt_reserve);
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &state,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    let transaction_accounts = |initial_balance: u64| -> Vec<(Pubkey, AccountSharedData)> {
        let split_to_account = AccountSharedData::new_data_with_space(
            initial_balance,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        vec![
            (stake_address, stake_account.clone()),
            (split_to_address, split_to_account),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (stake_history::id(), create_empty_stake_history_for_test()),
            (clock::id(), create_account_shared_data_for_test(&clock)),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ]
    };

    // Test insufficient account prefunding, including empty and less than rent_exempt_reserve.
    // The empty case is not covered in test_split, since that test uses a Meta with
    // rent_exempt_reserve = 0
    let split_lamport_balances = vec![0, rent_exempt_reserve - 1];
    for initial_balance in split_lamport_balances {
        let transaction_accounts = transaction_accounts(initial_balance);
        // split more than available fails
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );
        // split to insufficiently funded dest fails
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );
    }

    // Test various account prefunding, including exactly rent_exempt_reserve, and more than
    // rent_exempt_reserve
    let split_lamport_balances = vec![
        rent_exempt_reserve,
        rent_exempt_reserve + minimum_delegation - 1,
        rent_exempt_reserve + minimum_delegation,
    ];
    for initial_balance in split_lamport_balances {
        let transaction_accounts = transaction_accounts(initial_balance);
        let expected_active_stake = get_active_stake_for_tests(
            &[
                transaction_accounts[0].1.clone(),
                transaction_accounts[1].1.clone(),
            ],
            &clock,
            &StakeHistory::default(),
        );

        // split more than available fails
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );

        // should work
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Ok(()),
        );
        // no lamport leakage
        assert_eq!(
            accounts[0].lamports() + accounts[1].lamports(),
            stake_lamports + initial_balance,
        );
        // no deactivated stake
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &StakeHistory::default())
        );

        if let StakeStateV2::Stake(meta, stake, stake_flags) = state {
            let expected_stake =
                stake_lamports / 2 - (rent_exempt_reserve.saturating_sub(initial_balance));
            assert_eq!(
                Ok(StakeStateV2::Stake(
                    meta,
                    Stake {
                        delegation: Delegation {
                            stake: stake_lamports / 2
                                - (rent_exempt_reserve.saturating_sub(initial_balance)),
                            ..stake.delegation
                        },
                        ..stake
                    },
                    stake_flags
                )),
                accounts[1].state(),
            );
            assert_eq!(
                accounts[1].lamports(),
                expected_stake
                    + rent_exempt_reserve
                    + initial_balance.saturating_sub(rent_exempt_reserve),
            );
            assert_eq!(
                Ok(StakeStateV2::Stake(
                    meta,
                    Stake {
                        delegation: Delegation {
                            stake: stake_lamports / 2 - rent_exempt_reserve,
                            ..stake.delegation
                        },
                        ..stake
                    },
                    stake_flags,
                )),
                accounts[0].state(),
            );
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_from_larger_sized_account(mollusk: Mollusk) {
    let rent = Rent::default();
    let source_larger_rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of() + 100);
    let split_rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = (source_larger_rent_exempt_reserve + minimum_delegation) * 2;
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        authorized: Authorized::auto(&stake_address),
        rent_exempt_reserve: source_larger_rent_exempt_reserve,
        ..Meta::default()
    };
    let state = just_stake(meta, stake_lamports - source_larger_rent_exempt_reserve);
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &state,
        StakeStateV2::size_of() + 100,
        &id(),
    )
    .unwrap();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    let transaction_accounts = |initial_balance: u64| -> Vec<(Pubkey, AccountSharedData)> {
        let split_to_account = AccountSharedData::new_data_with_space(
            initial_balance,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        vec![
            (stake_address, stake_account.clone()),
            (split_to_address, split_to_account),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (stake_history::id(), create_empty_stake_history_for_test()),
            (clock::id(), create_account_shared_data_for_test(&clock)),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ]
    };

    // Test insufficient account prefunding, including empty and less than rent_exempt_reserve
    let split_lamport_balances = vec![0, split_rent_exempt_reserve - 1];
    for initial_balance in split_lamport_balances {
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts(initial_balance),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );
    }

    // Test various account prefunding, including exactly rent_exempt_reserve, and more than
    // rent_exempt_reserve. The empty case is not covered in test_split, since that test uses a
    // Meta with rent_exempt_reserve = 0
    let split_lamport_balances = vec![
        split_rent_exempt_reserve,
        split_rent_exempt_reserve + minimum_delegation - 1,
        split_rent_exempt_reserve + minimum_delegation,
    ];
    for initial_balance in split_lamport_balances {
        let transaction_accounts = transaction_accounts(initial_balance);
        let expected_active_stake = get_active_stake_for_tests(
            &[
                transaction_accounts[0].1.clone(),
                transaction_accounts[1].1.clone(),
            ],
            &clock,
            &StakeHistory::default(),
        );

        // split more than available fails
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InsufficientFunds),
        );

        // should work
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        // no lamport leakage
        assert_eq!(
            accounts[0].lamports() + accounts[1].lamports(),
            stake_lamports + initial_balance
        );
        // no deactivated stake
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &StakeHistory::default())
        );

        if let StakeStateV2::Stake(meta, stake, stake_flags) = state {
            let expected_split_meta = Meta {
                authorized: Authorized::auto(&stake_address),
                rent_exempt_reserve: split_rent_exempt_reserve,
                ..Meta::default()
            };
            let expected_stake =
                stake_lamports / 2 - (split_rent_exempt_reserve.saturating_sub(initial_balance));

            assert_eq!(
                Ok(StakeStateV2::Stake(
                    expected_split_meta,
                    Stake {
                        delegation: Delegation {
                            stake: expected_stake,
                            ..stake.delegation
                        },
                        ..stake
                    },
                    stake_flags,
                )),
                accounts[1].state()
            );
            assert_eq!(
                accounts[1].lamports(),
                expected_stake
                    + split_rent_exempt_reserve
                    + initial_balance.saturating_sub(split_rent_exempt_reserve)
            );
            assert_eq!(
                Ok(StakeStateV2::Stake(
                    meta,
                    Stake {
                        delegation: Delegation {
                            stake: stake_lamports / 2 - source_larger_rent_exempt_reserve,
                            ..stake.delegation
                        },
                        ..stake
                    },
                    stake_flags,
                )),
                accounts[0].state()
            );
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_from_smaller_sized_account(mollusk: Mollusk) {
    let rent = Rent::default();
    let source_smaller_rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let split_rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of() + 100);
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let stake_lamports = split_rent_exempt_reserve + 1;
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        authorized: Authorized::auto(&stake_address),
        rent_exempt_reserve: source_smaller_rent_exempt_reserve,
        ..Meta::default()
    };
    let state = just_stake(meta, stake_lamports - source_smaller_rent_exempt_reserve);
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &state,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    let split_amount = stake_lamports - (source_smaller_rent_exempt_reserve + 1); // Enough so that split stake is > 0
    let split_lamport_balances = vec![
        0,
        1,
        split_rent_exempt_reserve,
        split_rent_exempt_reserve + 1,
    ];
    for initial_balance in split_lamport_balances {
        let split_to_account = AccountSharedData::new_data_with_space(
            initial_balance,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of() + 100,
            &id(),
        )
        .unwrap();
        let transaction_accounts = vec![
            (stake_address, stake_account.clone()),
            (split_to_address, split_to_account),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (
                stake_history::id(),
                create_account_shared_data_for_test(&stake_history),
            ),
            (
                clock::id(),
                create_account_shared_data_for_test(&Clock {
                    epoch: current_epoch,
                    ..Clock::default()
                }),
            ),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ];

        // should always return error when splitting to larger account
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::InvalidAccountData),
        );

        // Splitting 100% of source should not make a difference
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Err(ProgramError::InvalidAccountData),
        );
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_100_percent_of_source(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = rent_exempt_reserve + minimum_delegation;
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        authorized: Authorized::auto(&stake_address),
        rent_exempt_reserve,
        ..Meta::default()
    };
    let split_to_address = solana_sdk::pubkey::new_rand();
    let split_to_account = AccountSharedData::new_data_with_space(
        0,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    // test splitting both an Initialized stake and a Staked stake
    for state in &[
        StakeStateV2::Initialized(meta),
        just_stake(meta, stake_lamports - rent_exempt_reserve),
    ] {
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let expected_active_stake = get_active_stake_for_tests(
            &[stake_account.clone(), split_to_account.clone()],
            &clock,
            &stake_history,
        );
        let transaction_accounts = vec![
            (stake_address, stake_account),
            (split_to_address, split_to_account.clone()),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (
                stake_history::id(),
                create_account_shared_data_for_test(&stake_history),
            ),
            (clock::id(), create_account_shared_data_for_test(&clock)),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ];

        // split 100% over to dest
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Ok(()),
        );

        // no lamport leakage
        assert_eq!(
            accounts[0].lamports() + accounts[1].lamports(),
            stake_lamports
        );
        // no deactivated stake
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &stake_history)
        );

        match state {
            StakeStateV2::Initialized(_) => {
                assert_eq!(Ok(*state), accounts[1].state());
                assert_eq!(Ok(StakeStateV2::Uninitialized), accounts[0].state());
            }
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                assert_eq!(
                    Ok(StakeStateV2::Stake(
                        *meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports - rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..*stake
                        },
                        *stake_flags
                    )),
                    accounts[1].state()
                );
                assert_eq!(Ok(StakeStateV2::Uninitialized), accounts[0].state());
            }
            _ => unreachable!(),
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_100_percent_of_source_to_account_with_lamports(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = rent_exempt_reserve + minimum_delegation;
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        authorized: Authorized::auto(&stake_address),
        rent_exempt_reserve,
        ..Meta::default()
    };
    let state = just_stake(meta, stake_lamports - rent_exempt_reserve);
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &state,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let split_to_address = solana_sdk::pubkey::new_rand();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
    // rent_exempt_reserve, and more than rent_exempt_reserve. Technically, the empty case is
    // covered in test_split_100_percent_of_source, but included here as well for readability
    let split_lamport_balances = vec![
        0,
        rent_exempt_reserve - 1,
        rent_exempt_reserve,
        rent_exempt_reserve + minimum_delegation - 1,
        rent_exempt_reserve + minimum_delegation,
    ];
    for initial_balance in split_lamport_balances {
        let split_to_account = AccountSharedData::new_data_with_space(
            initial_balance,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let expected_active_stake = get_active_stake_for_tests(
            &[stake_account.clone(), split_to_account.clone()],
            &clock,
            &stake_history,
        );
        let transaction_accounts = vec![
            (stake_address, stake_account.clone()),
            (split_to_address, split_to_account),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (
                stake_history::id(),
                create_account_shared_data_for_test(&stake_history),
            ),
            (clock::id(), create_account_shared_data_for_test(&clock)),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ];

        // split 100% over to dest
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Ok(()),
        );

        // no lamport leakage
        assert_eq!(
            accounts[0].lamports() + accounts[1].lamports(),
            stake_lamports + initial_balance
        );
        // no deactivated stake
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &stake_history)
        );

        if let StakeStateV2::Stake(meta, stake, stake_flags) = state {
            assert_eq!(
                Ok(StakeStateV2::Stake(
                    meta,
                    Stake {
                        delegation: Delegation {
                            stake: stake_lamports - rent_exempt_reserve,
                            ..stake.delegation
                        },
                        ..stake
                    },
                    stake_flags,
                )),
                accounts[1].state()
            );
            assert_eq!(Ok(StakeStateV2::Uninitialized), accounts[0].state());
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_rent_exemptness(mollusk: Mollusk) {
    let rent = Rent::default();
    let source_rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of() + 100);
    let split_rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_history = StakeHistory::default();
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let minimum_delegation = crate::get_minimum_delegation();
    let stake_lamports = source_rent_exempt_reserve + minimum_delegation;
    let stake_address = solana_sdk::pubkey::new_rand();
    let meta = Meta {
        authorized: Authorized::auto(&stake_address),
        rent_exempt_reserve: source_rent_exempt_reserve,
        ..Meta::default()
    };
    let split_to_address = solana_sdk::pubkey::new_rand();
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: split_to_address,
            is_signer: false,
            is_writable: true,
        },
    ];

    for state in &[
        StakeStateV2::Initialized(meta),
        just_stake(meta, stake_lamports - source_rent_exempt_reserve),
    ] {
        // Test that splitting to a larger account fails
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of() + 10000,
            &id(),
        )
        .unwrap();
        let transaction_accounts = vec![
            (stake_address, stake_account),
            (split_to_address, split_to_account),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (
                stake_history::id(),
                create_account_shared_data_for_test(&stake_history),
            ),
            (clock::id(), create_account_shared_data_for_test(&clock)),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ];
        process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Err(ProgramError::InvalidAccountData),
        );

        // Test that splitting from a larger account to a smaller one works.
        // Split amount should not matter, assuming other fund criteria are met
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            StakeStateV2::size_of() + 100,
            &id(),
        )
        .unwrap();
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        let expected_active_stake = get_active_stake_for_tests(
            &[stake_account.clone(), split_to_account.clone()],
            &clock,
            &stake_history,
        );
        let transaction_accounts = vec![
            (stake_address, stake_account),
            (split_to_address, split_to_account),
            (rent::id(), create_account_shared_data_for_test(&rent)),
            (
                stake_history::id(),
                create_account_shared_data_for_test(&stake_history),
            ),
            (
                clock::id(),
                create_account_shared_data_for_test(&Clock {
                    epoch: current_epoch,
                    ..Clock::default()
                }),
            ),
            (
                epoch_schedule::id(),
                create_account_shared_data_for_test(&EpochSchedule::default()),
            ),
        ];
        let accounts = process_instruction(
            &mollusk,
            &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(accounts[1].lamports(), stake_lamports);
        assert_eq!(
            expected_active_stake,
            get_active_stake_for_tests(&accounts[0..2], &clock, &stake_history)
        );

        let expected_split_meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve: split_rent_exempt_reserve,
            ..Meta::default()
        };
        match state {
            StakeStateV2::Initialized(_) => {
                assert_eq!(
                    Ok(StakeStateV2::Initialized(expected_split_meta)),
                    accounts[1].state()
                );
                assert_eq!(Ok(StakeStateV2::Uninitialized), accounts[0].state());
            }
            StakeStateV2::Stake(_meta, stake, stake_flags) => {
                // Expected stake should reflect original stake amount so that extra lamports
                // from the rent_exempt_reserve inequality do not magically activate
                let expected_stake = stake_lamports - source_rent_exempt_reserve;

                assert_eq!(
                    Ok(StakeStateV2::Stake(
                        expected_split_meta,
                        Stake {
                            delegation: Delegation {
                                stake: expected_stake,
                                ..stake.delegation
                            },
                            ..*stake
                        },
                        *stake_flags,
                    )),
                    accounts[1].state()
                );
                assert_eq!(
                    accounts[1].lamports(),
                    expected_stake + source_rent_exempt_reserve,
                );
                assert_eq!(Ok(StakeStateV2::Uninitialized), accounts[0].state());
            }
            _ => unreachable!(),
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_split_require_rent_exempt_destination(mollusk: Mollusk) {
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let current_epoch = 100;
    let clock = Clock {
        epoch: current_epoch,
        ..Clock::default()
    };
    let minimum_delegation = crate::get_minimum_delegation();
    let delegation_amount = 3 * minimum_delegation;
    let source_lamports = rent_exempt_reserve + delegation_amount;
    let source_address = Pubkey::new_unique();
    let destination_address = Pubkey::new_unique();
    let meta = Meta {
        authorized: Authorized::auto(&source_address),
        rent_exempt_reserve,
        ..Meta::default()
    };
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: source_address,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: destination_address,
            is_signer: false,
            is_writable: true,
        },
    ];
    let expected_result = Err(ProgramError::InsufficientFunds);

    for (split_amount, expected_result) in [
        (2 * minimum_delegation, expected_result),
        (source_lamports, Ok(())),
    ] {
        for (state, expected_result) in &[
            (StakeStateV2::Initialized(meta), Ok(())),
            (just_stake(meta, delegation_amount), expected_result),
        ] {
            let source_account = AccountSharedData::new_data_with_space(
                source_lamports,
                &state,
                StakeStateV2::size_of(),
                &id(),
            )
            .unwrap();

            let transaction_accounts = |initial_balance: u64| -> Vec<(Pubkey, AccountSharedData)> {
                let destination_account = AccountSharedData::new_data_with_space(
                    initial_balance,
                    &StakeStateV2::Uninitialized,
                    StakeStateV2::size_of(),
                    &id(),
                )
                .unwrap();
                vec![
                    (source_address, source_account.clone()),
                    (destination_address, destination_account),
                    (rent::id(), create_account_shared_data_for_test(&rent)),
                    (stake_history::id(), create_empty_stake_history_for_test()),
                    (clock::id(), create_account_shared_data_for_test(&clock)),
                    (
                        epoch_schedule::id(),
                        create_account_shared_data_for_test(&EpochSchedule::default()),
                    ),
                ]
            };

            // Test insufficient recipient prefunding
            let split_lamport_balances = vec![0, rent_exempt_reserve - 1];
            for initial_balance in split_lamport_balances {
                let transaction_accounts = transaction_accounts(initial_balance);
                let expected_active_stake = get_active_stake_for_tests(
                    &[source_account.clone(), transaction_accounts[1].1.clone()],
                    &clock,
                    &StakeHistory::default(),
                );
                let result_accounts = process_instruction(
                    &mollusk,
                    &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
                    transaction_accounts.clone(),
                    instruction_accounts.clone(),
                    if initial_balance + split_amount < rent_exempt_reserve {
                        Err(ProgramError::InsufficientFunds)
                    } else {
                        expected_result.clone()
                    },
                );
                let result_active_stake = get_active_stake_for_tests(
                    &result_accounts[0..2],
                    &clock,
                    &StakeHistory::default(),
                );
                if expected_active_stake > 0 // starting stake was delegated
                    // partial split
                    && result_accounts[0].lamports() > 0
                    // successful split to deficient recipient
                    && expected_result.is_ok()
                {
                    assert_ne!(expected_active_stake, result_active_stake);
                } else {
                    assert_eq!(expected_active_stake, result_active_stake);
                }
            }

            // Test recipient prefunding, including exactly rent_exempt_reserve, and more than
            // rent_exempt_reserve.
            let split_lamport_balances = vec![rent_exempt_reserve, rent_exempt_reserve + 1];
            for initial_balance in split_lamport_balances {
                let transaction_accounts = transaction_accounts(initial_balance);
                let expected_active_stake = get_active_stake_for_tests(
                    &[source_account.clone(), transaction_accounts[1].1.clone()],
                    &clock,
                    &StakeHistory::default(),
                );
                let accounts = process_instruction(
                    &mollusk,
                    &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
                    transaction_accounts,
                    instruction_accounts.clone(),
                    Ok(()),
                );

                // no lamport leakage
                assert_eq!(
                    accounts[0].lamports() + accounts[1].lamports(),
                    source_lamports + initial_balance
                );

                // no deactivated stake
                assert_eq!(
                    expected_active_stake,
                    get_active_stake_for_tests(&accounts[0..2], &clock, &StakeHistory::default())
                );

                if let StakeStateV2::Stake(meta, stake, stake_flags) = state {
                    // split entire source account, including rent-exempt reserve
                    if accounts[0].lamports() == 0 {
                        assert_eq!(Ok(StakeStateV2::Uninitialized), accounts[0].state());
                        assert_eq!(
                            Ok(StakeStateV2::Stake(
                                *meta,
                                Stake {
                                    delegation: Delegation {
                                        // delegated amount should not include source
                                        // rent-exempt reserve
                                        stake: delegation_amount,
                                        ..stake.delegation
                                    },
                                    ..*stake
                                },
                                *stake_flags,
                            )),
                            accounts[1].state()
                        );
                    } else {
                        assert_eq!(
                            Ok(StakeStateV2::Stake(
                                *meta,
                                Stake {
                                    delegation: Delegation {
                                        stake: minimum_delegation,
                                        ..stake.delegation
                                    },
                                    ..*stake
                                },
                                *stake_flags,
                            )),
                            accounts[0].state()
                        );
                        assert_eq!(
                            Ok(StakeStateV2::Stake(
                                *meta,
                                Stake {
                                    delegation: Delegation {
                                        stake: split_amount,
                                        ..stake.delegation
                                    },
                                    ..*stake
                                },
                                *stake_flags,
                            )),
                            accounts[1].state()
                        );
                    }
                }
            }
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_merge(mollusk: Mollusk) {
    let stake_address = solana_sdk::pubkey::new_rand();
    let merge_from_address = solana_sdk::pubkey::new_rand();
    let authorized_address = solana_sdk::pubkey::new_rand();
    let meta = Meta::auto(&authorized_address);
    let stake_lamports = 42;
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: merge_from_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authorized_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    for state in &[
        StakeStateV2::Initialized(meta),
        just_stake(meta, stake_lamports),
    ] {
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        for merge_from_state in &[
            StakeStateV2::Initialized(meta),
            just_stake(meta, stake_lamports),
        ] {
            let merge_from_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                merge_from_state,
                StakeStateV2::size_of(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account.clone()),
                (merge_from_address, merge_from_account),
                (authorized_address, AccountSharedData::default()),
                (
                    clock::id(),
                    create_account_shared_data_for_test(&Clock::default()),
                ),
                (
                    stake_history::id(),
                    create_account_shared_data_for_test(&StakeHistory::default()),
                ),
                (
                    epoch_schedule::id(),
                    create_account_shared_data_for_test(&EpochSchedule::default()),
                ),
            ];

            // Authorized staker signature required...
            instruction_accounts[4].is_signer = false;
            process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::Merge).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(ProgramError::MissingRequiredSignature),
            );
            instruction_accounts[4].is_signer = true;

            let accounts = process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::Merge).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Ok(()),
            );

            // check lamports
            assert_eq!(accounts[0].lamports(), stake_lamports * 2);
            assert_eq!(accounts[1].lamports(), 0);

            // check state
            match state {
                StakeStateV2::Initialized(meta) => {
                    assert_eq!(accounts[0].state(), Ok(StakeStateV2::Initialized(*meta)),);
                }
                StakeStateV2::Stake(meta, stake, stake_flags) => {
                    let expected_stake = stake.delegation.stake
                        + merge_from_state
                            .stake()
                            .map(|stake| stake.delegation.stake)
                            .unwrap_or_else(|| {
                                stake_lamports
                                    - merge_from_state.meta().unwrap().rent_exempt_reserve
                            });
                    assert_eq!(
                        accounts[0].state(),
                        Ok(StakeStateV2::Stake(
                            *meta,
                            Stake {
                                delegation: Delegation {
                                    stake: expected_stake,
                                    ..stake.delegation
                                },
                                ..*stake
                            },
                            *stake_flags,
                        )),
                    );
                }
                _ => unreachable!(),
            }
            assert_eq!(accounts[1].state(), Ok(StakeStateV2::Uninitialized));
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_merge_self_fails(mollusk: Mollusk) {
    let stake_address = solana_sdk::pubkey::new_rand();
    let authorized_address = solana_sdk::pubkey::new_rand();
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_amount = 4242424242;
    let stake_lamports = rent_exempt_reserve + stake_amount;
    let meta = Meta {
        rent_exempt_reserve,
        ..Meta::auto(&authorized_address)
    };
    let stake = Stake {
        delegation: Delegation {
            stake: stake_amount,
            activation_epoch: 0,
            ..Delegation::default()
        },
        ..Stake::default()
    };
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let transaction_accounts = vec![
        (stake_address, stake_account),
        (authorized_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authorized_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Merge).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(ProgramError::InvalidArgument),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_merge_incorrect_authorized_staker(mollusk: Mollusk) {
    let stake_address = solana_sdk::pubkey::new_rand();
    let merge_from_address = solana_sdk::pubkey::new_rand();
    let authorized_address = solana_sdk::pubkey::new_rand();
    let wrong_authorized_address = solana_sdk::pubkey::new_rand();
    let stake_lamports = 42;
    let mut instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: merge_from_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authorized_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    for state in &[
        StakeStateV2::Initialized(Meta::auto(&authorized_address)),
        just_stake(Meta::auto(&authorized_address), stake_lamports),
    ] {
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        for merge_from_state in &[
            StakeStateV2::Initialized(Meta::auto(&wrong_authorized_address)),
            just_stake(Meta::auto(&wrong_authorized_address), stake_lamports),
        ] {
            let merge_from_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                merge_from_state,
                StakeStateV2::size_of(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account.clone()),
                (merge_from_address, merge_from_account),
                (authorized_address, AccountSharedData::default()),
                (wrong_authorized_address, AccountSharedData::default()),
                (
                    clock::id(),
                    create_account_shared_data_for_test(&Clock::default()),
                ),
                (
                    stake_history::id(),
                    create_account_shared_data_for_test(&StakeHistory::default()),
                ),
                (
                    epoch_schedule::id(),
                    create_account_shared_data_for_test(&EpochSchedule::default()),
                ),
            ];

            instruction_accounts[4].pubkey = wrong_authorized_address;
            process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::Merge).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(ProgramError::MissingRequiredSignature),
            );
            instruction_accounts[4].pubkey = authorized_address;

            process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::Merge).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Err(StakeError::MergeMismatch.into()),
            );
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_merge_invalid_account_data(mollusk: Mollusk) {
    let stake_address = solana_sdk::pubkey::new_rand();
    let merge_from_address = solana_sdk::pubkey::new_rand();
    let authorized_address = solana_sdk::pubkey::new_rand();
    let stake_lamports = 42;
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: merge_from_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authorized_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    for state in &[
        StakeStateV2::Uninitialized,
        StakeStateV2::RewardsPool,
        StakeStateV2::Initialized(Meta::auto(&authorized_address)),
        just_stake(Meta::auto(&authorized_address), stake_lamports),
    ] {
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            state,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap();
        for merge_from_state in &[StakeStateV2::Uninitialized, StakeStateV2::RewardsPool] {
            let merge_from_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                merge_from_state,
                StakeStateV2::size_of(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account.clone()),
                (merge_from_address, merge_from_account),
                (authorized_address, AccountSharedData::default()),
                (
                    clock::id(),
                    create_account_shared_data_for_test(&Clock::default()),
                ),
                (
                    stake_history::id(),
                    create_account_shared_data_for_test(&StakeHistory::default()),
                ),
                (
                    epoch_schedule::id(),
                    create_account_shared_data_for_test(&EpochSchedule::default()),
                ),
            ];

            process_instruction(
                &mollusk,
                &serialize(&StakeInstruction::Merge).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Err(ProgramError::InvalidAccountData),
            );
        }
    }
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_merge_fake_stake_source(mollusk: Mollusk) {
    let stake_address = solana_sdk::pubkey::new_rand();
    let merge_from_address = solana_sdk::pubkey::new_rand();
    let authorized_address = solana_sdk::pubkey::new_rand();
    let stake_lamports = 42;
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &just_stake(Meta::auto(&authorized_address), stake_lamports),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let merge_from_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &just_stake(Meta::auto(&authorized_address), stake_lamports),
        StakeStateV2::size_of(),
        &solana_sdk::pubkey::new_rand(),
    )
    .unwrap();
    let transaction_accounts = vec![
        (stake_address, stake_account),
        (merge_from_address, merge_from_account),
        (authorized_address, AccountSharedData::default()),
        (
            clock::id(),
            create_account_shared_data_for_test(&Clock::default()),
        ),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&StakeHistory::default()),
        ),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: merge_from_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authorized_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    process_instruction(
        &mollusk,
        &serialize(&StakeInstruction::Merge).unwrap(),
        transaction_accounts,
        instruction_accounts,
        Err(if mollusk.is_bpf() {
            ProgramError::InvalidAccountOwner
        } else {
            ProgramError::IncorrectProgramId
        }),
    );
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_merge_active_stake(mollusk: Mollusk) {
    let stake_address = solana_sdk::pubkey::new_rand();
    let merge_from_address = solana_sdk::pubkey::new_rand();
    let authorized_address = solana_sdk::pubkey::new_rand();
    let base_lamports = 4242424242;
    let rent = Rent::default();
    let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
    let stake_amount = base_lamports;
    let stake_lamports = rent_exempt_reserve + stake_amount;
    let merge_from_amount = base_lamports;
    let merge_from_lamports = rent_exempt_reserve + merge_from_amount;
    let meta = Meta {
        rent_exempt_reserve,
        ..Meta::auto(&authorized_address)
    };
    let mut stake = Stake {
        delegation: Delegation {
            stake: stake_amount,
            activation_epoch: 0,
            ..Delegation::default()
        },
        ..Stake::default()
    };
    let stake_account = AccountSharedData::new_data_with_space(
        stake_lamports,
        &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let merge_from_activation_epoch = 2;
    let mut merge_from_stake = Stake {
        delegation: Delegation {
            stake: merge_from_amount,
            activation_epoch: merge_from_activation_epoch,
            ..stake.delegation
        },
        ..stake
    };
    let merge_from_account = AccountSharedData::new_data_with_space(
        merge_from_lamports,
        &StakeStateV2::Stake(meta, merge_from_stake, StakeFlags::empty()),
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();
    let mut clock = Clock::default();
    let mut stake_history = StakeHistory::default();
    let mut effective = base_lamports;
    let mut activating = stake_amount;
    let mut deactivating = 0;
    stake_history.add(
        clock.epoch,
        StakeHistoryEntry {
            effective,
            activating,
            deactivating,
        },
    );
    let mut transaction_accounts = vec![
        (stake_address, stake_account),
        (merge_from_address, merge_from_account),
        (authorized_address, AccountSharedData::default()),
        (clock::id(), create_account_shared_data_for_test(&clock)),
        (
            stake_history::id(),
            create_account_shared_data_for_test(&stake_history),
        ),
        (
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        ),
    ];
    let instruction_accounts = vec![
        AccountMeta {
            pubkey: stake_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: merge_from_address,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: clock::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: stake_history::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: authorized_address,
            is_signer: true,
            is_writable: false,
        },
    ];

    fn try_merge(
        mollusk: &Mollusk,
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        mut instruction_accounts: Vec<AccountMeta>,
        expected_result: Result<(), ProgramError>,
    ) {
        for iteration in 0..2 {
            if iteration == 1 {
                instruction_accounts.swap(0, 1);
            }
            let accounts = process_instruction(
                mollusk,
                &serialize(&StakeInstruction::Merge).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                expected_result.clone(),
            );
            if expected_result.is_ok() {
                assert_eq!(
                    accounts[1 - iteration].state(),
                    Ok(StakeStateV2::Uninitialized)
                );
            }
        }
    }

    // stake activation epoch, source initialized succeeds
    try_merge(
        &mollusk,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    let new_warmup_cooldown_rate_epoch = Some(0);

    // both activating fails
    loop {
        clock.epoch += 1;
        if clock.epoch == merge_from_activation_epoch {
            activating += merge_from_amount;
        }
        let delta = activating.min(
            (effective as f64 * warmup_cooldown_rate(clock.epoch, new_warmup_cooldown_rate_epoch))
                as u64,
        );
        effective += delta;
        activating -= delta;
        stake_history.add(
            clock.epoch - 1,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );
        transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));
        transaction_accounts[4] = (
            stake_history::id(),
            create_account_shared_data_for_test(&stake_history),
        );
        if stake_amount == stake.stake(clock.epoch, &stake_history, new_warmup_cooldown_rate_epoch)
            && merge_from_amount
                == merge_from_stake.stake(
                    clock.epoch,
                    &stake_history,
                    new_warmup_cooldown_rate_epoch,
                )
        {
            break;
        }
        try_merge(
            &mollusk,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::from(StakeError::MergeTransientStake)),
        );
    }

    // Both fully activated works
    try_merge(
        &mollusk,
        transaction_accounts.clone(),
        instruction_accounts.clone(),
        Ok(()),
    );

    // deactivate setup for deactivation
    let merge_from_deactivation_epoch = clock.epoch + 1;
    let stake_deactivation_epoch = clock.epoch + 2;

    // active/deactivating and deactivating/inactive mismatches fail
    loop {
        clock.epoch += 1;
        let delta = deactivating.min(
            (effective as f64 * warmup_cooldown_rate(clock.epoch, new_warmup_cooldown_rate_epoch))
                as u64,
        );
        effective -= delta;
        deactivating -= delta;
        if clock.epoch == stake_deactivation_epoch {
            deactivating += stake_amount;
            stake = Stake {
                delegation: Delegation {
                    deactivation_epoch: stake_deactivation_epoch,
                    ..stake.delegation
                },
                ..stake
            };
            transaction_accounts[0]
                .1
                .set_state(&StakeStateV2::Stake(meta, stake, StakeFlags::empty()))
                .unwrap();
        }
        if clock.epoch == merge_from_deactivation_epoch {
            deactivating += merge_from_amount;
            merge_from_stake = Stake {
                delegation: Delegation {
                    deactivation_epoch: merge_from_deactivation_epoch,
                    ..merge_from_stake.delegation
                },
                ..merge_from_stake
            };
            transaction_accounts[1]
                .1
                .set_state(&StakeStateV2::Stake(
                    meta,
                    merge_from_stake,
                    StakeFlags::empty(),
                ))
                .unwrap();
        }
        stake_history.add(
            clock.epoch - 1,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );
        transaction_accounts[3] = (clock::id(), create_account_shared_data_for_test(&clock));
        transaction_accounts[4] = (
            stake_history::id(),
            create_account_shared_data_for_test(&stake_history),
        );
        if 0 == stake.stake(clock.epoch, &stake_history, new_warmup_cooldown_rate_epoch)
            && 0 == merge_from_stake.stake(
                clock.epoch,
                &stake_history,
                new_warmup_cooldown_rate_epoch,
            )
        {
            break;
        }
        try_merge(
            &mollusk,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(ProgramError::from(StakeError::MergeTransientStake)),
        );
    }

    // Both fully deactivated works
    try_merge(&mollusk, transaction_accounts, instruction_accounts, Ok(()));
}

#[test_case(mollusk_native(); "native_stake")]
#[test_case(mollusk_bpf(); "bpf_stake")]
fn test_stake_get_minimum_delegation(mollusk: Mollusk) {
    let stake_address = Pubkey::new_unique();
    let stake_account = create_default_stake_account();
    let minimum_delegation = crate::get_minimum_delegation();
    let instruction_data = serialize(&StakeInstruction::GetMinimumDelegation).unwrap();
    let transaction_accounts = vec![(stake_address, stake_account)]
        .into_iter()
        .map(|(key, account)| (key, account.into()))
        .collect::<Vec<_>>();
    let instruction_accounts = vec![AccountMeta {
        pubkey: stake_address,
        is_signer: false,
        is_writable: true,
    }];

    let instruction = Instruction {
        program_id: id(),
        accounts: instruction_accounts,
        data: instruction_data,
    };

    mollusk.process_and_validate_instruction(
        &instruction,
        &transaction_accounts,
        &[
            Check::success(),
            Check::return_data(&minimum_delegation.to_le_bytes()),
        ],
    );
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
            mollusk,
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
