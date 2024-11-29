#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::arithmetic_side_effects)]

use {
    mollusk_svm::{result::Check, Mollusk},
    solana_account::{AccountSharedData, WritableAccount},
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
            instruction::{
                self as ixn, LockupArgs, LockupCheckedArgs, StakeError, StakeInstruction,
            },
            state::{
                warmup_cooldown_rate, Authorized, Delegation, Lockup, Meta, Stake,
                StakeActivationStatus, StakeAuthorize, StakeStateV2,
            },
        },
        stake_history::StakeHistoryEntry,
        system_instruction, system_program,
        sysvar::{
            clock::Clock, epoch_schedule::EpochSchedule, rent::Rent, stake_history::StakeHistory,
            SysvarId,
        },
        transaction::{Transaction, TransactionError},
        vote::{
            program as vote_program,
            state::{VoteInit, VoteState, VoteStateVersions},
        },
    },
    solana_stake_program::{id, processor::Processor},
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

// arbitrary, but gives us room to set up activations/deactivations serveral epochs in the past
const EXECUTION_EPOCH: u64 = 8;

// mollusk doesnt charge transaction fees, this is just a convenient source of lamports
const PAYER: Pubkey = Pubkey::from_str_const("PAYER7y5empWisbHsxHGE7vgBWKZCemGo1gKw7NpQSK");
const PAYER_BALANCE: u64 = 1_000_000 * LAMPORTS_PER_SOL;

// two vote accounts with no credits, fine for all stake tests except DeactivateDelinquent
const VOTE_ACCOUNT_RED: Pubkey =
    Pubkey::from_str_const("REDjn6cyjcZkXAvRHWFtAd4chwHd6MmtqT2u965cDqg");
const VOTE_ACCOUNT_BLUE: Pubkey =
    Pubkey::from_str_const("BLUE7fsMB69ti5fDRZEbZVoWdXbCoA8bwk963vXsZs7");

// two blank stake accounts that can be serialized into for tests
const STAKE_ACCOUNT_BLACK: Pubkey =
    Pubkey::from_str_const("BLACK8oXP6Ar933gupyVZqunKYmmb8rEnrbPSqpxbFt");
const STAKE_ACCOUNT_WHITE: Pubkey =
    Pubkey::from_str_const("WH1TE3e9czGF33AtbkTBbQ4BQ3EY7BaL8utApeYfSnL");

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

    fn resolve_accounts(&self, account_metas: &[AccountMeta]) -> Vec<(Pubkey, AccountSharedData)> {
        let mut accounts = vec![];
        for account_meta in account_metas {
            let key = account_meta.pubkey;
            let account_shared_data = if Rent::check_id(&key) {
                self.mollusk.sysvars.keyed_account_for_rent_sysvar().1
            } else {
                self.accounts.get(&key).cloned().unwrap()
            };

            accounts.push((key, account_shared_data));
        }

        accounts
    }
}

fn stake_to_bytes(stake: &StakeStateV2) -> Vec<u8> {
    let mut data = vec![0; StakeStateV2::size_of()];
    bincode::serialize_into(&mut data[..], stake).unwrap();
    data
}

#[test]
fn test_initialize() {
    let env = Env::init();

    let authorized = Authorized::default();
    let lockup = Lockup::default();

    let instruction = ixn::initialize(&STAKE_ACCOUNT_BLACK, &authorized, &lockup);
    let accounts = env.resolve_accounts(&instruction.accounts);

    let black_state = StakeStateV2::Initialized(Meta {
        rent_exempt_reserve: STAKE_RENT_EXEMPTION,
        authorized,
        lockup,
    });
    let black = stake_to_bytes(&black_state);

    env.mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[
            Check::success(),
            Check::account(&STAKE_ACCOUNT_BLACK).data(&black).build(),
        ],
    );
}
