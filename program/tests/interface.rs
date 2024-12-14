#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::arithmetic_side_effects)]

use {
    arbitrary::{Arbitrary, Unstructured},
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
    solana_sdk::{
        account::Account as SolanaAccount,
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
        },
        stake_history::StakeHistoryEntry,
        system_instruction, system_program,
        sysvar::{
            clock::Clock, epoch_rewards::EpochRewards, epoch_schedule::EpochSchedule, rent::Rent,
            stake_history::StakeHistory, SysvarId,
        },
        transaction::{Transaction, TransactionError},
        vote::{
            program as vote_program,
            state::{VoteInit, VoteState, VoteStateVersions},
        },
    },
    solana_stake_program::{get_minimum_delegation, id, processor::Processor},
    std::{
        collections::{HashMap, HashSet},
        fs,
        sync::LazyLock,
    },
    test_case::{test_case, test_matrix},
};

// arbitrary, gives us room to set up activations/deactivations
const EXECUTION_EPOCH: u64 = 8;

// mollusk doesnt charge transaction fees, this is just a convenient source/sink for lamports
const PAYER: Pubkey = Pubkey::from_str_const("PAYER11111111111111111111111111111111111111");
const PAYER_BALANCE: u64 = 1_000_000 * LAMPORTS_PER_SOL;

// two vote accounts with no credits, fine for all stake tests except DeactivateDelinquent
const VOTE_ACCOUNT_RED: Pubkey =
    Pubkey::from_str_const("RED1111111111111111111111111111111111111111");
const VOTE_ACCOUNT_BLUE: Pubkey =
    Pubkey::from_str_const("BLUE111111111111111111111111111111111111111");

// two blank stake accounts that can be serialized into for tests
const STAKE_ACCOUNT_BLACK: Pubkey =
    Pubkey::from_str_const("BLACK11111111111111111111111111111111111111");
const STAKE_ACCOUNT_WHITE: Pubkey =
    Pubkey::from_str_const("WH1TE11111111111111111111111111111111111111");

// separate authorities for two stake accounts
const STAKER_BLACK: Pubkey = Pubkey::from_str_const("STAKERBLACK11111111111111111111111111111111");
const WITHDRAWER_BLACK: Pubkey =
    Pubkey::from_str_const("W1THDRAWERBLACK1111111111111111111111111111");
const STAKER_WHITE: Pubkey = Pubkey::from_str_const("STAKERWH1TE11111111111111111111111111111111");
const WITHDRAWER_WHITE: Pubkey =
    Pubkey::from_str_const("W1THDRAWERWH1TE1111111111111111111111111111");

// shared authorities for two stake accounts, clearly distinguished from the above
const STAKER_GRAY: Pubkey = Pubkey::from_str_const("STAKERGRAY111111111111111111111111111111111");
const WITHDRAWER_GRAY: Pubkey =
    Pubkey::from_str_const("W1THDRAWERGRAY11111111111111111111111111111");

// valid custodians for any stake account
const CUSTODIAN_LEFT: Pubkey =
    Pubkey::from_str_const("CUSTXD1ANLEFT111111111111111111111111111111");
const CUSTODIAN_RIGHT: Pubkey =
    Pubkey::from_str_const("CUSTXD1ANR1GHT11111111111111111111111111111");

// stake delegated to some imaginary vote account in all epochs
// with a warmup/cooldown rate of 9%, routine tests moving under 9sol can ignore stake history
// while also making it easy to write tests involving partial (de)activations
// if the warmup/cooldown rate changes, this number must be adjusted
const PERSISTANT_ACTIVE_STAKE: u64 = 100 * LAMPORTS_PER_SOL;
#[test]
fn assert_warmup_cooldown_rate() {
    assert_eq!(warmup_cooldown_rate(0, Some(0)), 0.09);
}

// hardcoded for convenience
const STAKE_RENT_EXEMPTION: u64 = 2_282_880;
#[test]
fn assert_stake_rent_exemption() {
    assert_eq!(
        Rent::default().minimum_balance(StakeStateV2::size_of()),
        STAKE_RENT_EXEMPTION
    );
}

// exhaustive set of all test instruction declarations
// this is probabalistic but should exceed ten nines
// implementing it by hand would be extremely annoying
static INSTRUCTION_DECLARATIONS: LazyLock<HashSet<StakeInterface>> = LazyLock::new(|| {
    let mut declarations = HashSet::new();
    for _ in 0..10_000 {
        let raw_data: Vec<u8> = (0..StakeInterface::max_size())
            .map(|_| rand::random::<u8>())
            .collect();
        let mut unstructured = Unstructured::new(&raw_data);
        declarations.insert(StakeInterface::arbitrary(&mut unstructured).unwrap());
    }

    declarations
});

// we use two hashmaps because cloning mollusk is impossible and creating it is expensive
// doing this we let base_accounts be immutable and can set and clear override_accounts
struct Env {
    mollusk: Mollusk,
    base_accounts: HashMap<Pubkey, Account>,
    override_accounts: HashMap<Pubkey, Account>,
}
impl Env {
    // set up a test environment with valid stake history, two vote accounts, and two blank stake accounts
    fn init() -> Self {
        // create a test environment at the execution epoch
        let mut base_accounts = HashMap::new();
        let mut mollusk = Mollusk::new(&id(), "solana_stake_program");
        solana_logger::setup_with("");
        mollusk.warp_to_slot(EXECUTION_EPOCH * mollusk.sysvars.epoch_schedule.slots_per_epoch + 1);
        assert_eq!(mollusk.sysvars.clock.epoch, EXECUTION_EPOCH);

        // backfill stake history
        for epoch in 0..EXECUTION_EPOCH {
            mollusk.sysvars.stake_history.add(
                epoch,
                StakeHistoryEntry::with_effective(PERSISTANT_ACTIVE_STAKE),
            );
        }

        // add a lamports source
        let payer_account =
            Account::new_rent_epoch(PAYER_BALANCE, 0, &system_program::id(), u64::MAX);
        base_accounts.insert(PAYER, payer_account);

        // create two vote accounts
        let vote_rent_exemption = Rent::default().minimum_balance(VoteState::size_of());
        let vote_state_versions = VoteStateVersions::new_current(VoteState::default());
        let vote_data = bincode::serialize(&vote_state_versions).unwrap();
        let vote_account = Account::create(
            vote_rent_exemption,
            vote_data,
            vote_program::id(),
            false,
            u64::MAX,
        );
        base_accounts.insert(VOTE_ACCOUNT_RED, vote_account.clone());
        base_accounts.insert(VOTE_ACCOUNT_BLUE, vote_account);

        // create two blank stake accounts
        let stake_account = Account::create(
            STAKE_RENT_EXEMPTION,
            vec![0; StakeStateV2::size_of()],
            id(),
            false,
            u64::MAX,
        );
        base_accounts.insert(STAKE_ACCOUNT_BLACK, stake_account.clone());
        base_accounts.insert(STAKE_ACCOUNT_WHITE, stake_account);

        Self {
            mollusk,
            base_accounts,
            override_accounts: HashMap::new(),
        }
    }

    // set up one of the preconfigured blank stake accounts at some starting state
    // to mutate the accounts after initial setup, do it directly or execute instructions
    // note these accounts are already rent exempt, so lamports specified are stake or extra
    fn update_stake(
        &mut self,
        pubkey: &Pubkey,
        stake_state: &StakeStateV2,
        additional_lamports: u64,
    ) {
        assert!(*pubkey == STAKE_ACCOUNT_BLACK || *pubkey == STAKE_ACCOUNT_WHITE);

        let mut stake_account = if let Some(stake_account) = self.override_accounts.get(pubkey) {
            stake_account.clone()
        } else {
            self.base_accounts.get(pubkey).cloned().unwrap()
        };

        let current_lamports = stake_account.lamports();
        stake_account.set_lamports(current_lamports + additional_lamports);
        bincode::serialize_into(stake_account.data_as_mut_slice(), stake_state).unwrap();

        self.override_accounts.insert(*pubkey, stake_account);
    }

    // get the accounts from our account store that this transaction expects to see
    // we dont need implicit sysvars, mollusk resolves them internally via syscall stub
    fn resolve_accounts(&self, account_metas: &[AccountMeta]) -> Vec<(Pubkey, AccountSharedData)> {
        let mut accounts = vec![];
        for account_meta in account_metas {
            let key = account_meta.pubkey;
            let account_shared_data = if Rent::check_id(&key) {
                self.mollusk.sysvars.keyed_account_for_rent_sysvar().1
            } else if Clock::check_id(&key) {
                self.mollusk.sysvars.keyed_account_for_clock_sysvar().1
            } else if EpochSchedule::check_id(&key) {
                self.mollusk
                    .sysvars
                    .keyed_account_for_epoch_schedule_sysvar()
                    .1
            } else if EpochRewards::check_id(&key) {
                self.mollusk
                    .sysvars
                    .keyed_account_for_epoch_rewards_sysvar()
                    .1
            } else if StakeHistory::check_id(&key) {
                self.mollusk
                    .sysvars
                    .keyed_account_for_stake_history_sysvar()
                    .1
            } else if let Some(account) = self.override_accounts.get(&key).cloned() {
                account.into()
            } else if let Some(account) = self.base_accounts.get(&key).cloned() {
                account.into()
            } else {
                AccountSharedData::default()
            };

            accounts.push((key, account_shared_data));
        }

        accounts
    }

    /* XXX
        // process an instruction, assert checks, and update override accounts
        fn process(&mut self, instruction: &Instruction, checks: &[Check]) {
            let initial_accounts = self.resolve_accounts(&instruction.accounts);

            let result =
                self.mollusk
                    .process_and_validate_instruction(instruction, &initial_accounts, checks);

            for (i, resulting_account) in result.resulting_accounts.into_iter().enumerate() {
                let account_meta = &instruction.accounts[i];
                assert_eq!(account_meta.pubkey, resulting_account.0);
                if account_meta.is_writable {
                    if resulting_account.1.lamports() == 0 {
                        self.override_accounts.remove(&resulting_account.0);
                    } else {
                        self.override_accounts
                            .insert(resulting_account.0, resulting_account.1);
                    }
                }
            }
        }
    */

    // immutable process with only a success check
    fn process_success(&self, instruction: &Instruction) {
        let accounts = self.resolve_accounts(&instruction.accounts);
        //println!("HANA ixn: {:#?}\n     accts: {:#?}\n    hm1: {:#?}\n    hm2: {:#?}", instruction, accounts, self.base_accounts, self.override_accounts);
        self.mollusk
            .process_and_validate_instruction(instruction, &accounts, &[Check::success()]);
    }

    // immutable process with an expected error
    fn process_fail(&self, instruction: &Instruction, error: ProgramError) {
        let accounts = self.resolve_accounts(&instruction.accounts);
        self.mollusk
            .process_and_validate_instruction(instruction, &accounts, &[Check::err(error)]);
    }

    fn reset(&mut self) {
        self.override_accounts.clear()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum StakeInterface {
    Initialize(LockupState),
    Authorize(AuthorityType, LockupState),
    DelegateStake(LockupState),
    /*
    Split(AmountFraction),
    Withdraw(LockupState, AmountFraction),
    Deactivate(AuthorityType),
    SetLockup(LockupState, LockupState),
    Merge(AuthorityType),
    */
    // TODO move, checked, seed, deactivate delinquent, minimum, redelegate
}

impl StakeInterface {
    // unfortunately `size_hint()` is useless
    // we substantially overshoot to avoid mistakes
    fn max_size() -> usize {
        32
    }

    // creates an instruction with the given combination of settings that is guaranteed to succeed
    fn to_instruction(&self, env: &mut Env) -> Instruction {
        let minimum_delegation = get_minimum_delegation();

        match self {
            Self::Initialize(lockup_state) => instruction::initialize(
                &STAKE_ACCOUNT_BLACK,
                &Authorized {
                    staker: STAKER_BLACK,
                    withdrawer: WITHDRAWER_BLACK,
                },
                &lockup_state.to_lockup(CUSTODIAN_LEFT),
            ),
            Self::Authorize(authority_type, lockup_state) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &not_just_stake(
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        false,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                let authorize = authority_type.into();
                let (old_authority, new_authority) = match authorize {
                    StakeAuthorize::Staker => (STAKER_BLACK, STAKER_GRAY),
                    StakeAuthorize::Withdrawer => (WITHDRAWER_BLACK, WITHDRAWER_GRAY),
                };

                instruction::authorize(
                    &STAKE_ACCOUNT_BLACK,
                    &old_authority,
                    &new_authority,
                    authorize,
                    lockup_state.to_custodian(&CUSTODIAN_LEFT),
                )
            }
            Self::DelegateStake(lockup_state) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &not_just_stake(
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        false,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                instruction::delegate_stake(&STAKE_ACCOUNT_BLACK, &STAKER_BLACK, &VOTE_ACCOUNT_RED)
            }
            /*
            // TODO amount
            Self::Split(_) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation * 2,
                        true,
                    ),
                    minimum_delegation * 2,
                );

                instruction::split(
                    &STAKE_ACCOUNT_BLACK,
                    &STAKER_GRAY,
                    minimum_delegation,
                    &STAKE_ACCOUNT_WHITE,
                )[2]
                .clone()
            }
            // TODO partial, lockup
            Self::Withdraw(_) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        false,
                    ),
                    minimum_delegation,
                );

                instruction::withdraw(
                    &STAKE_ACCOUNT_BLACK,
                    &WITHDRAWER_BLACK,
                    &PAYER,
                    minimum_delegation + STAKE_RENT_EXEMPTION,
                    None,
                )
            }
            // TODO withdrawer
            Self::Deactivate => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        false,
                    ),
                    minimum_delegation,
                );

                instruction::deactivate_stake(&STAKE_ACCOUNT_BLACK, &STAKER_BLACK)
            }
            // TODO existing lockup, remove lockup, also hardcoded custodians maybe?
            Self::SetLockup(_) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &just_stake(STAKE_ACCOUNT_BLACK, minimum_delegation),
                    minimum_delegation,
                );

                instruction::set_lockup(
                    &STAKE_ACCOUNT_BLACK,
                    &LockupArgs {
                        epoch: Some(EXECUTION_EPOCH * 2),
                        custodian: Some(Pubkey::new_unique()),
                        unix_timestamp: None,
                    },
                    &WITHDRAWER_BLACK,
                )
            }
            // TODO withdrawer
            Self::Merge => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        true,
                    ),
                    minimum_delegation,
                );

                env.update_stake(
                    &STAKE_ACCOUNT_WHITE,
                    &active_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_WHITE,
                        minimum_delegation,
                        true,
                    ),
                    minimum_delegation,
                );

                instruction::merge(&STAKE_ACCOUNT_WHITE, &STAKE_ACCOUNT_BLACK, &STAKER_GRAY)[0]
                    .clone()
            }
            */
            // TODO move, checked, seed, deactivate delinquent, minimum, redelegate
            _ => todo!(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum AuthorityType {
    Staker,
    Withdrawer,
}

impl AuthorityType {
    fn pubkey(&self, stake_pubkey: Pubkey) -> Pubkey {
        match (stake_pubkey, self) {
            (STAKE_ACCOUNT_BLACK, Self::Staker) => STAKER_BLACK,
            (STAKE_ACCOUNT_BLACK, Self::Withdrawer) => WITHDRAWER_BLACK,
            (STAKE_ACCOUNT_WHITE, Self::Staker) => STAKER_WHITE,
            (STAKE_ACCOUNT_WHITE, Self::Withdrawer) => WITHDRAWER_WHITE,
            _ => panic!("expected a hardcoded stake pubkey, got {}", stake_pubkey),
        }
    }
}

impl From<&AuthorityType> for StakeAuthorize {
    fn from(authority_type: &AuthorityType) -> Self {
        match authority_type {
            AuthorityType::Staker => Self::Staker,
            AuthorityType::Withdrawer => Self::Withdrawer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum LockupState {
    Active,
    Inactive,
    None,
}

impl LockupState {
    fn to_lockup(&self, custodian: Pubkey) -> Lockup {
        match self {
            Self::Active => Lockup {
                custodian,
                epoch: EXECUTION_EPOCH + 1,
                unix_timestamp: 0,
            },
            Self::Inactive => Lockup {
                custodian,
                epoch: EXECUTION_EPOCH - 1,
                unix_timestamp: 0,
            },
            Self::None => Lockup::default(),
        }
    }

    fn to_custodian<'a>(&self, custodian: &'a Pubkey) -> Option<&'a Pubkey> {
        match self {
            Self::None => None,
            _ => Some(custodian),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum AmountFraction {
    Partial,
    Full,
}

fn just_stake(stake_pubkey: Pubkey, stake: u64) -> StakeStateV2 {
    not_just_stake(stake_pubkey, stake, false, Lockup::default())
}

fn not_just_stake(
    stake_pubkey: Pubkey,
    stake: u64,
    common_authority: bool,
    lockup: Lockup,
) -> StakeStateV2 {
    i_cant_believe_its_not_stake(
        Pubkey::default(),
        stake_pubkey,
        stake,
        common_authority,
        lockup,
        false,
    )
}

fn i_cant_believe_its_not_stake(
    voter_pubkey: Pubkey,
    stake_pubkey: Pubkey,
    stake: u64,
    common_authority: bool,
    lockup: Lockup,
    is_active: bool,
) -> StakeStateV2 {
    assert!(stake_pubkey != VOTE_ACCOUNT_RED);
    assert!(stake_pubkey != VOTE_ACCOUNT_BLUE);

    let authorized = match stake_pubkey {
        _ if common_authority => Authorized {
            staker: STAKER_GRAY,
            withdrawer: WITHDRAWER_GRAY,
        },
        STAKE_ACCOUNT_BLACK => Authorized {
            staker: STAKER_BLACK,
            withdrawer: WITHDRAWER_BLACK,
        },
        STAKE_ACCOUNT_WHITE => Authorized {
            staker: STAKER_WHITE,
            withdrawer: WITHDRAWER_WHITE,
        },
        _ => panic!("expected a hardcoded stake pubkey, got {}", stake_pubkey),
    };

    let activation_epoch = if is_active { EXECUTION_EPOCH - 1 } else { 0 };

    StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            authorized,
            lockup,
        },
        Stake {
            delegation: Delegation {
                stake,
                voter_pubkey,
                activation_epoch,
                ..Delegation::default()
            },
            ..Stake::default()
        },
        StakeFlags::empty(),
    )
}

fn stake_to_bytes(stake: &StakeStateV2) -> Vec<u8> {
    let mut data = vec![0; StakeStateV2::size_of()];
    bincode::serialize_into(&mut data[..], stake).unwrap();
    data
}

#[test]
fn test_all_success() {
    let mut env = Env::init();

    for declaration in &*INSTRUCTION_DECLARATIONS {
        let instruction = declaration.to_instruction(&mut env);
        env.process_success(&instruction);
        env.reset();
    }
}
