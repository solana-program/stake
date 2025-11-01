use {
    mollusk_svm::Mollusk,
    solana_account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{stake_history::StakeHistory, state::StakeStateV2},
    solana_sysvar_id::SysvarId,
    solana_vote_interface::state::{VoteStateV4, VoteStateVersions},
    std::collections::HashMap,
};

// hardcoded for convenience
pub const STAKE_RENT_EXEMPTION: u64 = 2_282_880;

#[test]
fn assert_stake_rent_exemption() {
    assert_eq!(
        Rent::default().minimum_balance(StakeStateV2::size_of()),
        STAKE_RENT_EXEMPTION
    );
}

/// Resolve all accounts for an instruction, including sysvars and instruction accounts
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
            mollusk.sysvars.keyed_account_for_stake_history_sysvar().1
        } else {
            // Default empty account
            Account::default()
        };

        result.push((key, account));
    }

    result
}

/// Create a vote account with VoteStateV4
pub fn create_vote_account() -> AccountSharedData {
    let space = VoteStateV4::size_of();
    let lamports = Rent::default().minimum_balance(space);
    let vote_state = VoteStateVersions::new_v4(VoteStateV4::default());
    let data = bincode::serialize(&vote_state).unwrap();

    Account::create(lamports, data, solana_sdk_ids::vote::id(), false, u64::MAX).into()
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
