#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::arithmetic_side_effects)]

use {
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, ReadableAccount, WritableAccount},
    solana_sdk::{
        account::Account as SolanaAccount,
        entrypoint::ProgramResult,
        feature_set::{move_stake_and_move_lamports_ixs, stake_raise_minimum_delegation_to_1_sol},
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
    std::collections::HashMap,
    test_case::{test_case, test_matrix},
};

// XXX ok so wow i am going to have to write a lot of shit
// we need a mechanism to create basically arbitrary stake accounts
// this means all states (uninit, init, activating, active, deactivating, deactive)
// we need to be able to make a stake history that gives us partial activation/deactivation
// actually we need to set up stake history ourselves correctly in all cases
// we need to be able to set lockup and authority arbitrarily
// we need helpers to set up with seed pubkeys
// ideally we automatically check missing signer failures
// need to create a vote account... ugh we need to get credits right for DeactivateDelinquent
// for delegate we just need owner, vote account pubkey, and credits (can be 0)
//
// XXX OK i wrote a simple init test
// what to do on monday... i guess go through the stake ixn tests and see what to impl
// main thing we lack is full coverage for lockup and i think a bunch of split edge cases

// arbitrary, but gives us room to set up activations/deactivations serveral epochs in the past
const EXECUTION_EPOCH: u64 = 8;

// mollusk doesnt charge transaction fees, this is just a convenient source of lamports
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

// authorities for tests which use separate ones
const STAKER_BLACK: Pubkey = Pubkey::from_str_const("STAKERBLACK11111111111111111111111111111111");
const WITHDRAWER_BLACK: Pubkey =
    Pubkey::from_str_const("W1THDRAWERBLACK1111111111111111111111111111");
const STAKER_WHITE: Pubkey = Pubkey::from_str_const("STAKERWH1TE11111111111111111111111111111111");
const WITHDRAWER_WHITE: Pubkey =
    Pubkey::from_str_const("W1THDRAWERWH1TE1111111111111111111111111111");

// authorities for tests which use shared ones
const STAKER_GRAY: Pubkey = Pubkey::from_str_const("STAKERGRAY111111111111111111111111111111111");
const WITHDRAWER_GRAY: Pubkey =
    Pubkey::from_str_const("W1THDRAWERGRAY11111111111111111111111111111");

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

struct Env {
    mollusk: Mollusk,
    accounts: HashMap<Pubkey, AccountSharedData>,
}
impl Env {
    // set up a test environment with valid stake history, two vote accounts, and two blank stake accounts
    fn init() -> Self {
        // create a test environment at the execution epoch
        let mut accounts = HashMap::new();
        let mut mollusk = Mollusk::new(&id(), "solana_stake_program");
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
        let payer_data =
            AccountSharedData::new_rent_epoch(PAYER_BALANCE, 0, &system_program::id(), u64::MAX);
        accounts.insert(PAYER, payer_data);

        // create two vote accounts
        let vote_rent_exemption = Rent::default().minimum_balance(VoteState::size_of());
        let vote_state = bincode::serialize(&VoteState::default()).unwrap();
        let vote_data = AccountSharedData::create(
            vote_rent_exemption,
            vote_state,
            vote_program::id(),
            false,
            u64::MAX,
        );
        accounts.insert(VOTE_ACCOUNT_RED, vote_data.clone());
        accounts.insert(VOTE_ACCOUNT_BLUE, vote_data);

        // create two blank stake accounts
        let stake_data = AccountSharedData::create(
            STAKE_RENT_EXEMPTION,
            vec![0; StakeStateV2::size_of()],
            id(),
            false,
            u64::MAX,
        );
        accounts.insert(STAKE_ACCOUNT_BLACK, stake_data.clone());
        accounts.insert(STAKE_ACCOUNT_WHITE, stake_data);

        Self { mollusk, accounts }
    }

    // creates a test environment and instruction for a given stake operation
    // enum contents are sometimes, but not necessarily, ignored
    // the success is trivial, this is mostly to allow exhaustive failure tests
    // or to do some post-setup for more meaningful success tests
    // XXX this is horrible. getting full coverage is fucking insane. this shit is too complicated
    // probably need my own enum... or at least add lockup as an arg for everything
    fn init_for_instruction(stake_instruction: &StakeInstruction) -> (Self, Instruction) {
        let mut env = Self::init();
        let minimum_delegation = get_minimum_delegation();

        let instruction = match stake_instruction {
            StakeInstruction::Initialize(_, _) => instruction::initialize(
                &STAKE_ACCOUNT_BLACK,
                &Authorized {
                    staker: STAKER_BLACK,
                    withdrawer: WITHDRAWER_BLACK,
                },
                &Lockup::default(),
            ),
            // TODO lockup
            StakeInstruction::Authorize(_, authorize) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &just_stake(STAKE_ACCOUNT_BLACK, minimum_delegation),
                    minimum_delegation,
                );

                let (old_authority, new_authority) = match authorize {
                    StakeAuthorize::Staker => (STAKER_BLACK, STAKER_GRAY),
                    StakeAuthorize::Withdrawer => (WITHDRAWER_BLACK, WITHDRAWER_GRAY),
                };

                instruction::authorize(
                    &STAKE_ACCOUNT_BLACK,
                    &old_authority,
                    &new_authority,
                    *authorize,
                    None,
                )
            }
            // TODO withdrawer
            StakeInstruction::DelegateStake => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &just_stake(STAKE_ACCOUNT_BLACK, minimum_delegation),
                    minimum_delegation,
                );

                instruction::delegate_stake(&STAKE_ACCOUNT_BLACK, &STAKER_BLACK, &VOTE_ACCOUNT_RED)
            }
            // TODO amount, also maybe should use gray
            StakeInstruction::Split(_) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(
                        VOTE_ACCOUNT_RED,
                        STAKE_ACCOUNT_BLACK,
                        minimum_delegation * 2,
                    ),
                    minimum_delegation * 2,
                );

                instruction::split(
                    &STAKE_ACCOUNT_BLACK,
                    &STAKER_BLACK,
                    minimum_delegation,
                    &STAKE_ACCOUNT_WHITE,
                )[2]
                .clone()
            }
            // TODO partial, lockup
            StakeInstruction::Withdraw(_) => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(VOTE_ACCOUNT_RED, STAKE_ACCOUNT_BLACK, minimum_delegation),
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
            StakeInstruction::Deactivate => {
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(VOTE_ACCOUNT_RED, STAKE_ACCOUNT_BLACK, minimum_delegation),
                    minimum_delegation,
                );

                instruction::deactivate_stake(&STAKE_ACCOUNT_BLACK, &STAKER_BLACK)
            }
            // TODO existing lockup, remove lockup, also hardcoded custodians maybe?
            StakeInstruction::SetLockup(_) => {
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
            StakeInstruction::Merge => {
                // XXX TODO FIXME these need to use gray
                env.update_stake(
                    &STAKE_ACCOUNT_BLACK,
                    &active_stake(VOTE_ACCOUNT_RED, STAKE_ACCOUNT_BLACK, minimum_delegation),
                    minimum_delegation,
                );

                env.update_stake(
                    &STAKE_ACCOUNT_WHITE,
                    &active_stake(VOTE_ACCOUNT_RED, STAKE_ACCOUNT_WHITE, minimum_delegation),
                    minimum_delegation,
                );

                instruction::merge(&STAKE_ACCOUNT_WHITE, &STAKE_ACCOUNT_BLACK, &STAKER_GRAY)[0]
                    .clone()
            }
            // TODO move, checked, seed, deactivate delinquent, minimum, redelegate
            _ => todo!(),
        };

        (env, instruction)
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
            } else if let Some(account) = self.accounts.get(&key).cloned() {
                account
            } else {
                AccountSharedData::default()
            };

            accounts.push((key, account_shared_data));
        }

        accounts
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
        let stake_account = self.accounts.get_mut(pubkey).unwrap();
        let current_lamports = stake_account.lamports();
        stake_account.set_lamports(current_lamports + additional_lamports);
        bincode::serialize_into(stake_account.data_as_mut_slice(), stake_state).unwrap();
    }

    // process an instruction, assert checks, and update internal accounts
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
                    self.accounts.remove(&resulting_account.0);
                } else {
                    self.accounts
                        .insert(resulting_account.0, resulting_account.1);
                }
            }
        }
    }

    // shorthand for process with only a success check
    fn process_success(&mut self, instruction: &Instruction) {
        self.process(instruction, &[Check::success()]);
    }

    // shorthand for process with an expected error
    fn process_fail(&mut self, instruction: &Instruction, error: ProgramError) {
        self.process(instruction, &[Check::err(error)]);
    }
}

fn just_stake(stake_pubkey: Pubkey, stake: u64) -> StakeStateV2 {
    let authorized = match stake_pubkey {
        STAKE_ACCOUNT_BLACK => Authorized {
            staker: STAKER_BLACK,
            withdrawer: WITHDRAWER_BLACK,
        },
        STAKE_ACCOUNT_WHITE => Authorized {
            staker: STAKER_WHITE,
            withdrawer: WITHDRAWER_WHITE,
        },
        _ => Authorized::default(),
    };

    StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            authorized,
            lockup: Lockup::default(),
        },
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

fn active_stake(voter_pubkey: Pubkey, stake_pubkey: Pubkey, stake: u64) -> StakeStateV2 {
    assert!(stake_pubkey != VOTE_ACCOUNT_RED);
    assert!(stake_pubkey != VOTE_ACCOUNT_BLUE);

    let authorized = match stake_pubkey {
        STAKE_ACCOUNT_BLACK => Authorized {
            staker: STAKER_BLACK,
            withdrawer: WITHDRAWER_BLACK,
        },
        STAKE_ACCOUNT_WHITE => Authorized {
            staker: STAKER_WHITE,
            withdrawer: WITHDRAWER_WHITE,
        },
        _ => Authorized::default(),
    };

    StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            authorized,
            lockup: Lockup::default(),
        },
        Stake {
            delegation: Delegation {
                stake,
                voter_pubkey,
                activation_epoch: EXECUTION_EPOCH - 1,
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
fn test_initialize() {
    let mut env = Env::init();

    let authorized = Authorized::default();
    let lockup = Lockup::default();

    let instruction = instruction::initialize(&STAKE_ACCOUNT_BLACK, &authorized, &lockup);

    let black_state = StakeStateV2::Initialized(Meta {
        rent_exempt_reserve: STAKE_RENT_EXEMPTION,
        authorized,
        lockup,
    });
    let black = stake_to_bytes(&black_state);

    env.process(
        &instruction,
        &[
            Check::success(),
            Check::account(&STAKE_ACCOUNT_BLACK).data(&black).build(),
        ],
    );
}
