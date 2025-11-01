use {
    mollusk_svm::Mollusk,
    solana_account::{Account, AccountSharedData},
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{stake_history::StakeHistory, state::StakeStateV2},
    solana_sysvar_id::SysvarId,
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
