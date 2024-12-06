#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::arithmetic_side_effects)]

use {
    bincode::serialize,
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, ReadableAccount, WritableAccount},
    solana_program_runtime::loaded_programs::ProgramCacheEntryOwner,
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
        native_loader,
        native_token::LAMPORTS_PER_SOL,
        program_error::ProgramError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        signers::Signers,
        stake::{
            self, config as stake_config,
            instruction::{
                self, authorize_checked, authorize_checked_with_seed, initialize_checked,
                set_lockup_checked, AuthorizeCheckedWithSeedArgs, AuthorizeWithSeedArgs,
                LockupArgs, StakeError, StakeInstruction,
            },
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
    },
    solana_stake_program::{get_minimum_delegation, id, processor::Processor},
    solana_vote_program::{
        self,
        vote_state::{self, VoteState, VoteStateVersions},
    },
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

fn process_instruction_as_one_arg(
    mollusk: &Mollusk,
    instruction: &Instruction,
    expected_result: Result<(), ProgramError>,
) -> Vec<AccountSharedData> {
    let transaction_accounts = get_default_transaction_accounts(instruction);
    process_instruction(
        &mollusk,
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

pub fn delegation_from(account: &AccountSharedData) -> Option<Delegation> {
    from(account).and_then(|state: StakeStateV2| state.delegation())
}

pub fn authorized_from(account: &AccountSharedData) -> Option<Authorized> {
    from(account).and_then(|state: StakeStateV2| state.authorized())
}

pub fn lockup_from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<Lockup> {
    from(account).and_then(|state: StakeStateV2| state.lockup())
}

pub fn meta_from(account: &AccountSharedData) -> Option<Meta> {
    from(account).and_then(|state: StakeStateV2| state.meta())
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

// XXX SKIP BEOFRE THIS
// the tests are kind of dumb but i mihgt grab them anyway
// just annoying bc they test errors that changed
// and are kind of useless, we should actually test the interface systematically

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

// XXX NOTE DIVIDE HERE
// below, i have everything up to the end of the file
// but working backwards is fucking annoying

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

// XXX SKIP test_stake_get_minimum_delegation

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
