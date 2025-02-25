#![allow(clippy::arithmetic_side_effects)]

use {
    arbitrary::{Arbitrary, Unstructured},
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{Account, ReadableAccount, WritableAccount},
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        stake::{
            instruction::{self, LockupArgs},
            stake_flags::StakeFlags,
            state::{
                warmup_cooldown_rate, Authorized, Delegation, Lockup, Meta, Stake, StakeAuthorize,
                StakeStateV2, NEW_WARMUP_COOLDOWN_RATE,
            },
        },
        stake_history::StakeHistoryEntry,
        system_program,
        sysvar::{
            clock::Clock, epoch_rewards::EpochRewards, epoch_schedule::EpochSchedule, rent::Rent,
            stake_history::StakeHistory, SysvarId,
        },
        vote::{
            program as vote_program,
            state::{VoteState, VoteStateVersions},
        },
    },
    solana_stake_program::{get_minimum_delegation, id},
    std::{
        collections::{HashMap, HashSet},
        sync::LazyLock,
    },
};

// StakeInterface encapsulates every combination of instruction, account states, and input parameters
// that we want to test, so that we can exhaustively generate valid, successful instructions. The
// output instructions can be used as-is to verify the program interface works for all combinations of
// input classes. They can also be changed to operations that must fail, to test error checks have no gaps.
//
// Env encapulates the mollusk test runner, a set of "base" accounts constituting the default state,
// and a set of "override" accounts which change that state prior to instruction execution.
// This is to allow us to repeatedly reuse one Env (by dropping the overrides and creating new ones)
// instead of creating it from scratch for each test, which would make these tests take minutes
// to run once we add more cases.
//
// All constants and addresses in this file are used with mollusk to set up base usable accounts,
// which StakeInterface then puts into a suitable state for particular instructions. For example,
// Merge sets up two stake accounts with appropriate lockups, authorities, and activation states.

// NOTE ideas for future tests:
// * fail with different vote accounts on operations that require them to match
// * fail with different authorities on operations that require them to match
// * adding/changing lockups to ensure we always fail when violating lockup

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
const PERSISTENT_ACTIVE_STAKE: u64 = 100 * LAMPORTS_PER_SOL;
#[test]
fn assert_warmup_cooldown_rate() {
    assert_eq!(warmup_cooldown_rate(0, Some(0)), NEW_WARMUP_COOLDOWN_RATE);
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
// this is probabilistic but should exceed ten nines
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
        mollusk.warp_to_slot(EXECUTION_EPOCH * mollusk.sysvars.epoch_schedule.slots_per_epoch + 1);
        assert_eq!(mollusk.sysvars.clock.epoch, EXECUTION_EPOCH);

        // backfill stake history
        let stake_delta_amount =
            (PERSISTENT_ACTIVE_STAKE as f64 * NEW_WARMUP_COOLDOWN_RATE).floor() as u64;
        for epoch in 0..EXECUTION_EPOCH {
            mollusk.sysvars.stake_history.add(
                epoch,
                StakeHistoryEntry {
                    effective: PERSISTENT_ACTIVE_STAKE,
                    activating: stake_delta_amount,
                    deactivating: stake_delta_amount,
                },
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
    fn resolve_accounts(&self, account_metas: &[AccountMeta]) -> Vec<(Pubkey, Account)> {
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
                account
            } else {
                self.base_accounts.get(&key).cloned().unwrap_or_default()
            };

            accounts.push((key, account_shared_data));
        }

        accounts
    }

    // immutable process that should succeed
    fn process_success(&self, instruction: &Instruction) {
        let accounts = self.resolve_accounts(&instruction.accounts);
        self.mollusk
            .process_and_validate_instruction(instruction, &accounts, &[Check::success()]);
    }

    // immutable process that should fail
    fn process_fail(&self, instruction: &Instruction) {
        let accounts = self.resolve_accounts(&instruction.accounts);
        let result = self.mollusk.process_instruction(instruction, &accounts);
        assert!(result.program_result.is_err());
    }

    fn reset(&mut self) {
        self.override_accounts.clear()
    }
}

// NOTE we skip:
// * redelegate: will never be enabled
// * minimum delegation: cannot fail in any nontrivial way
// * deactivate delinquent: requires no signers, and our only failure test is missing signers
// the first two need never be added but the third should be when we have more tests
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum StakeInterface {
    Initialize {
        lockup_state: LockupState,
    },
    InitializeChecked,
    Authorize {
        checked: bool,
        authority_type: AuthorityType,
        lockup_state: LockupState,
    },
    AuthorizeWithSeed {
        checked: bool,
        authority_type: AuthorityType,
        lockup_state: LockupState,
    },
    SetLockup {
        checked: bool,
        existing_lockup_state: LockupState,
        new_lockup_state: LockupState,
    },
    DelegateStake {
        lockup_state: LockupState,
    },
    Split {
        lockup_state: LockupState,
        full_split: bool,
    },
    Merge {
        lockup_state: LockupState,
    },
    MoveStake {
        lockup_state: LockupState,
        active_destination: bool,
        full_move: bool,
    },
    MoveLamports {
        lockup_state: LockupState,
        active_source: bool,
        destination_status: MoveLamportsStatus,
    },
    Withdraw {
        lockup_state: LockupState,
        source_status: WithdrawStatus,
        full_withdraw: bool,
    },
    Deactivate {
        lockup_state: LockupState,
    },
}

impl StakeInterface {
    // unfortunately `size_hint()` is useless
    // we substantially overshoot to avoid mistakes
    fn max_size() -> usize {
        128
    }

    // creates an instruction with the given combination of settings that is guaranteed to succeed
    fn to_instruction(self, env: &mut Env) -> Instruction {
        let minimum_delegation = get_minimum_delegation();

        match self {
            Self::Initialize { lockup_state } => instruction::initialize(
                &STAKE_ACCOUNT_BLACK,
                &Authorized {
                    staker: STAKER_BLACK,
                    withdrawer: WITHDRAWER_BLACK,
                },
                &lockup_state.to_lockup(CUSTODIAN_LEFT),
            ),
            Self::InitializeChecked => instruction::initialize_checked(
                &STAKE_ACCOUNT_BLACK,
                &Authorized {
                    staker: STAKER_BLACK,
                    withdrawer: WITHDRAWER_BLACK,
                },
            ),
            Self::Authorize {
                checked,
                authority_type,
                lockup_state,
            } => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &initialized_stake(
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

                let make_instruction = if checked {
                    instruction::authorize_checked
                } else {
                    instruction::authorize
                };

                make_instruction(
                    &STAKE_ACCOUNT_BLACK,
                    &old_authority,
                    &new_authority,
                    authorize,
                    lockup_state.to_custodian(&CUSTODIAN_LEFT),
                )
            }
            Self::AuthorizeWithSeed {
                checked,
                authority_type,
                lockup_state,
            } => {
                let seed_base = Pubkey::new_unique();
                let seed = "seed";
                let seed_authority =
                    Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();

                let mut black_state = initialized_stake(
                    STAKE_ACCOUNT_BLACK,
                    minimum_delegation,
                    false,
                    lockup_state.to_lockup(CUSTODIAN_LEFT),
                );

                let authorize = authority_type.into();
                let new_authority = match black_state {
                    StakeStateV2::Initialized(ref mut meta) => match authorize {
                        StakeAuthorize::Staker => {
                            meta.authorized.staker = seed_authority;
                            STAKER_GRAY
                        }
                        StakeAuthorize::Withdrawer => {
                            meta.authorized.withdrawer = seed_authority;
                            WITHDRAWER_GRAY
                        }
                    },
                    _ => unreachable!(),
                };

                env.update_stake(&STAKE_ACCOUNT_BLACK, &black_state, minimum_delegation);

                let make_instruction = if checked {
                    instruction::authorize_checked_with_seed
                } else {
                    instruction::authorize_with_seed
                };

                make_instruction(
                    &STAKE_ACCOUNT_BLACK,
                    &seed_base,
                    seed.to_string(),
                    &system_program::id(),
                    &new_authority,
                    authorize,
                    lockup_state.to_custodian(&CUSTODIAN_LEFT),
                )
            }
            Self::SetLockup {
                checked,
                existing_lockup_state,
                new_lockup_state,
            } => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &initialized_stake(
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        false,
                        existing_lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                let make_instruction = if checked {
                    instruction::set_lockup_checked
                } else {
                    instruction::set_lockup
                };

                make_instruction(
                    &STAKE_ACCOUNT_BLACK,
                    &new_lockup_state.to_args(CUSTODIAN_RIGHT),
                    existing_lockup_state
                        .to_custodian(&CUSTODIAN_LEFT)
                        .unwrap_or(&WITHDRAWER_BLACK),
                )
            }
            Self::DelegateStake { lockup_state } => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &initialized_stake(
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        false,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                instruction::delegate_stake(&STAKE_ACCOUNT_BLACK, &STAKER_BLACK, &VOTE_ACCOUNT_RED)
            }
            Self::Split {
                lockup_state,
                full_split,
            } => {
                let delegated_stake = minimum_delegation * 2;
                let split_amount = if full_split {
                    delegated_stake + STAKE_RENT_EXEMPTION
                } else {
                    delegated_stake / 2
                };

                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        delegated_stake,
                        StakeStatus::Active,
                        true,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    delegated_stake,
                );

                instruction::split(
                    &STAKE_ACCOUNT_BLACK,
                    &STAKER_GRAY,
                    split_amount,
                    &STAKE_ACCOUNT_WHITE,
                )
                .remove(2)
            }
            Self::Merge { lockup_state } => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        StakeStatus::Active,
                        true,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                env.update_stake(
                    &STAKE_ACCOUNT_WHITE,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_WHITE,
                        minimum_delegation,
                        StakeStatus::Active,
                        true,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                instruction::merge(&STAKE_ACCOUNT_WHITE, &STAKE_ACCOUNT_BLACK, &STAKER_GRAY)
                    .remove(0)
            }
            Self::MoveStake {
                lockup_state,
                active_destination,
                full_move,
            } => {
                let source_delegation = minimum_delegation * 2;
                let move_amount = if full_move {
                    source_delegation
                } else {
                    source_delegation / 2
                };

                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        source_delegation,
                        StakeStatus::Active,
                        true,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    source_delegation,
                );

                env.update_stake(
                    &STAKE_ACCOUNT_WHITE,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_WHITE,
                        minimum_delegation,
                        if active_destination {
                            StakeStatus::Active
                        } else {
                            StakeStatus::Initialized
                        },
                        true,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                instruction::move_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &STAKE_ACCOUNT_WHITE,
                    &STAKER_GRAY,
                    move_amount,
                )
            }
            Self::MoveLamports {
                lockup_state,
                active_source,
                destination_status,
            } => {
                let free_lamports = LAMPORTS_PER_SOL;

                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        if active_source {
                            StakeStatus::Active
                        } else {
                            StakeStatus::Initialized
                        },
                        true,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation + free_lamports,
                );

                env.update_stake(
                    &STAKE_ACCOUNT_WHITE,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_WHITE,
                        minimum_delegation,
                        destination_status.into(),
                        true,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                instruction::move_lamports(
                    &STAKE_ACCOUNT_BLACK,
                    &STAKE_ACCOUNT_WHITE,
                    &STAKER_GRAY,
                    free_lamports,
                )
            }
            Self::Withdraw {
                lockup_state,
                full_withdraw,
                source_status,
            } => {
                let free_lamports = LAMPORTS_PER_SOL;
                let source_status = source_status.into();

                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        source_status,
                        false,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation + free_lamports,
                );

                let withdraw_amount = if full_withdraw && source_status != StakeStatus::Active {
                    free_lamports + minimum_delegation + STAKE_RENT_EXEMPTION
                } else {
                    free_lamports
                };

                let authority = if source_status == StakeStatus::Uninitialized {
                    STAKE_ACCOUNT_BLACK
                } else {
                    WITHDRAWER_BLACK
                };

                instruction::withdraw(
                    &STAKE_ACCOUNT_BLACK,
                    &authority,
                    &PAYER,
                    withdraw_amount,
                    lockup_state.to_custodian(&CUSTODIAN_LEFT),
                )
            }
            Self::Deactivate { lockup_state } => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &fully_configurable_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation,
                        StakeStatus::Active,
                        false,
                        lockup_state.to_lockup(CUSTODIAN_LEFT),
                    ),
                    minimum_delegation,
                );

                instruction::deactivate_stake(&STAKE_ACCOUNT_BLACK, &STAKER_BLACK)
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum StakeStatus {
    Uninitialized,
    Initialized,
    Activating,
    Active,
    Deactivating,
    Deactive,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum MoveLamportsStatus {
    Initialized,
    Activating,
    Active,
}

impl From<MoveLamportsStatus> for StakeStatus {
    fn from(status: MoveLamportsStatus) -> Self {
        match status {
            MoveLamportsStatus::Initialized => StakeStatus::Initialized,
            MoveLamportsStatus::Activating => StakeStatus::Activating,
            MoveLamportsStatus::Active => StakeStatus::Active,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum WithdrawStatus {
    Uninitialized,
    Initialized,
    Active,
}

impl From<WithdrawStatus> for StakeStatus {
    fn from(status: WithdrawStatus) -> Self {
        match status {
            WithdrawStatus::Uninitialized => StakeStatus::Uninitialized,
            WithdrawStatus::Initialized => StakeStatus::Initialized,
            WithdrawStatus::Active => StakeStatus::Active,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum AuthorityType {
    Staker,
    Withdrawer,
}

impl From<AuthorityType> for StakeAuthorize {
    fn from(authority_type: AuthorityType) -> Self {
        match authority_type {
            AuthorityType::Staker => Self::Staker,
            AuthorityType::Withdrawer => Self::Withdrawer,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Arbitrary)]
enum LockupState {
    Active,
    Inactive,
    None,
}

impl LockupState {
    fn to_lockup(self, custodian: Pubkey) -> Lockup {
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

    fn to_custodian(self, custodian: &Pubkey) -> Option<&Pubkey> {
        match self {
            Self::Active => Some(custodian),
            _ => None,
        }
    }

    fn to_args(self, custodian: Pubkey) -> LockupArgs {
        match self {
            Self::None => LockupArgs::default(),
            _ => LockupArgs {
                custodian: self.to_custodian(&custodian).cloned(),
                epoch: Some(self.to_lockup(custodian).epoch),
                unix_timestamp: None,
            },
        }
    }
}

// initialized with settable authority and lockup
fn initialized_stake(
    stake_pubkey: Pubkey,
    stake: u64,
    use_gray_authority: bool,
    lockup: Lockup,
) -> StakeStateV2 {
    fully_configurable_stake(
        Pubkey::default(),
        stake_pubkey,
        stake,
        StakeStatus::Initialized,
        use_gray_authority,
        lockup,
    )
}

// any point in the stake lifecycle with settable vote account, authority, and lockup
fn fully_configurable_stake(
    voter_pubkey: Pubkey,
    stake_pubkey: Pubkey,
    stake: u64,
    stake_status: StakeStatus,
    use_gray_authority: bool,
    lockup: Lockup,
) -> StakeStateV2 {
    assert!(stake_pubkey != VOTE_ACCOUNT_RED);
    assert!(stake_pubkey != VOTE_ACCOUNT_BLUE);

    let authorized = match stake_pubkey {
        _ if use_gray_authority => Authorized {
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

    let meta = Meta {
        rent_exempt_reserve: STAKE_RENT_EXEMPTION,
        authorized,
        lockup,
    };

    let delegation = match stake_status {
        StakeStatus::Uninitialized | StakeStatus::Initialized => Delegation::default(),
        StakeStatus::Activating => Delegation {
            stake,
            voter_pubkey,
            activation_epoch: EXECUTION_EPOCH,
            ..Delegation::default()
        },
        StakeStatus::Active => Delegation {
            stake,
            voter_pubkey,
            activation_epoch: EXECUTION_EPOCH - 1,
            ..Delegation::default()
        },
        StakeStatus::Deactivating => Delegation {
            stake,
            voter_pubkey,
            activation_epoch: EXECUTION_EPOCH - 1,
            deactivation_epoch: EXECUTION_EPOCH,
            ..Delegation::default()
        },
        StakeStatus::Deactive => Delegation {
            stake,
            voter_pubkey,
            activation_epoch: EXECUTION_EPOCH - 2,
            deactivation_epoch: EXECUTION_EPOCH - 1,
            ..Delegation::default()
        },
    };

    match stake_status {
        StakeStatus::Uninitialized => StakeStateV2::Uninitialized,
        StakeStatus::Initialized => StakeStateV2::Initialized(meta),
        _ => StakeStateV2::Stake(
            meta,
            Stake {
                delegation,
                ..Stake::default()
            },
            StakeFlags::empty(),
        ),
    }
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

#[test]
fn test_no_signer_bypass() {
    let mut env = Env::init();

    for declaration in &*INSTRUCTION_DECLARATIONS {
        let instruction = declaration.to_instruction(&mut env);
        for i in 0..instruction.accounts.len() {
            if !instruction.accounts[i].is_signer {
                continue;
            }

            let mut instruction = instruction.clone();
            instruction.accounts[i].is_signer = false;
            env.process_fail(&instruction);
            env.reset();
        }
    }
}
