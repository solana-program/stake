use {
    super::{lifecycle::StakeLifecycle, stake_tracker::StakeTracker},
    mollusk_svm::Mollusk,
    solana_account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
    solana_clock::Epoch,
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{
        instruction as ixn,
        stake_history::StakeHistory,
        state::{Authorized, Lockup, StakeStateV2},
    },
    solana_stake_program::id,
    solana_sysvar_id::SysvarId,
    solana_vote_interface::state::{VoteStateV4, VoteStateVersions},
    std::collections::HashMap,
};

// Hardcoded for convenience - matches interface.rs
pub const STAKE_RENT_EXEMPTION: u64 = 2_282_880;

#[test]
fn assert_stake_rent_exemption() {
    assert_eq!(
        Rent::default().minimum_balance(StakeStateV2::size_of()),
        STAKE_RENT_EXEMPTION
    );
}

/// Create a vote account with VoteStateV4
pub fn create_vote_account() -> AccountSharedData {
    let space = VoteStateV4::size_of();
    let lamports = Rent::default().minimum_balance(space);
    let vote_state = VoteStateVersions::new_v4(VoteStateV4::default());
    let data = bincode::serialize(&vote_state).unwrap();

    Account::create(lamports, data, solana_sdk_ids::vote::id(), false, u64::MAX).into()
}

/// Increment vote account credits
pub fn increment_vote_account_credits(
    vote_account: &mut AccountSharedData,
    epoch: Epoch,
    credits: u64,
) {
    let mut vote_state: VoteStateVersions = bincode::deserialize(vote_account.data()).unwrap();

    if let VoteStateVersions::V4(ref mut v4) = vote_state {
        v4.epoch_credits.push((epoch, credits, 0));
    }

    vote_account.set_data(bincode::serialize(&vote_state).unwrap());
}

/// Get the effective stake for an account
pub fn get_effective_stake(mollusk: &Mollusk, stake_account: &AccountSharedData) -> u64 {
    let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();

    if let StakeStateV2::Stake(_, stake, _) = stake_state {
        stake
            .delegation
            .stake_activating_and_deactivating(
                mollusk.sysvars.clock.epoch,
                &mollusk.sysvars.stake_history,
                Some(0),
            )
            .effective
    } else {
        0
    }
}

/// Parse a stake account into (Meta, Option<Stake>, lamports)
pub fn parse_stake_account(
    stake_account: &AccountSharedData,
) -> (
    solana_stake_interface::state::Meta,
    Option<solana_stake_interface::state::Stake>,
    u64,
) {
    let lamports = stake_account.lamports();
    let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();

    match stake_state {
        StakeStateV2::Initialized(meta) => (meta, None, lamports),
        StakeStateV2::Stake(meta, stake, _) => (meta, Some(stake), lamports),
        _ => panic!("Expected initialized or staked account"),
    }
}

/// Resolve all accounts for an instruction, including sysvars and instruction accounts
/// This follows the pattern from interface.rs
///
/// This function re-serializes the stake history sysvar from mollusk.sysvars.stake_history
/// every time it's called, ensuring that any updates to the stake history are reflected in the accounts.
pub fn add_sysvars(
    mollusk: &Mollusk,
    instruction: &Instruction,
    accounts: Vec<(Pubkey, AccountSharedData)>,
) -> Vec<(Pubkey, Account)> {
    // Build a map of provided accounts
    let mut account_map: HashMap<Pubkey, Account> = accounts
        .into_iter()
        .map(|(pk, acc)| (pk, acc.into()))
        .collect();

    // Now resolve all accounts from the instruction
    let mut result = Vec::new();
    for account_meta in &instruction.accounts {
        let key = account_meta.pubkey;
        let account = if let Some(acc) = account_map.remove(&key) {
            // Use the provided account
            acc
        } else if Rent::check_id(&key) {
            mollusk.sysvars.keyed_account_for_rent_sysvar().1
        } else if solana_clock::Clock::check_id(&key) {
            mollusk.sysvars.keyed_account_for_clock_sysvar().1
        } else if solana_epoch_schedule::EpochSchedule::check_id(&key) {
            mollusk.sysvars.keyed_account_for_epoch_schedule_sysvar().1
        } else if solana_epoch_rewards::EpochRewards::check_id(&key) {
            mollusk.sysvars.keyed_account_for_epoch_rewards_sysvar().1
        } else if StakeHistory::check_id(&key) {
            // Re-serialize stake history from mollusk.sysvars.stake_history
            // to ensure updates are reflected
            mollusk.sysvars.keyed_account_for_stake_history_sysvar().1
        } else {
            // Default empty account
            // Note: stake_config is not provided, so get_minimum_delegation() returns 1
            Account::default()
        };

        result.push((key, account));
    }

    result
}

/// Initialize a stake account with the given authorities and lockup
/// This is a convenience helper that creates the uninitialized account
/// and processes the instruction in one step.
pub fn initialize_stake_account(
    mollusk: &Mollusk,
    stake_pubkey: &Pubkey,
    lamports: u64,
    authorized: &Authorized,
    lockup: &Lockup,
) -> AccountSharedData {
    let stake_account = AccountSharedData::new_data_with_space(
        lamports,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let instruction = ixn::initialize(stake_pubkey, authorized, lockup);
    let accounts = vec![(*stake_pubkey, stake_account)];
    let accounts_resolved = add_sysvars(mollusk, &instruction, accounts);
    let result = mollusk.process_instruction(&instruction, &accounts_resolved);

    result.resulting_accounts[0].1.clone().into()
}

/// Synchronize a transient stake's epoch to the current epoch
/// Updates both the account data and the tracker.
pub fn true_up_transient_stake_epoch(
    mollusk: &mut Mollusk,
    tracker: &mut StakeTracker,
    stake_pubkey: &Pubkey,
    stake_account: &mut AccountSharedData,
    lifecycle: StakeLifecycle,
) {
    if lifecycle != StakeLifecycle::Activating && lifecycle != StakeLifecycle::Deactivating {
        return;
    }

    let clock = mollusk.sysvars.clock.clone();
    let mut stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();

    if let StakeStateV2::Stake(_, ref mut stake, _) = &mut stake_state {
        match lifecycle {
            StakeLifecycle::Activating => {
                stake.delegation.activation_epoch = clock.epoch;

                // Update tracker as well
                if let Some(tracked) = tracker.delegations.get_mut(stake_pubkey) {
                    tracked.activation_epoch = clock.epoch;
                }
            }
            StakeLifecycle::Deactivating => {
                stake.delegation.deactivation_epoch = clock.epoch;

                // Update tracker as well
                tracker.track_deactivation(stake_pubkey, clock.epoch);
            }
            _ => (),
        }
    }
    stake_account.set_data(bincode::serialize(&stake_state).unwrap());
}
