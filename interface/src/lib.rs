//! The Stake program interface.

#![cfg_attr(feature = "frozen-abi", feature(min_specialization))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

use solana_pubkey::Pubkey;

#[allow(deprecated)]
pub mod config;
pub mod error;
pub mod instruction;
pub mod stake_flags;
pub mod stake_history;
pub mod state;
pub mod tools;

pub mod program {
    solana_pubkey::declare_id!("Stake11111111111111111111111111111111111111");
}

/// The minimum number of epochs before stake account that is delegated to a delinquent vote
/// account may be unstaked with `StakeInstruction::DeactivateDelinquent`
pub const MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION: usize = 5;

// Inline some constants to avoid dependencies.
//
// Note: replace these inline IDs with the corresponding value from
// `solana_sdk_ids` once the version is updated to 2.2.0.

const CLOCK_ID: Pubkey = Pubkey::from_str_const("SysvarC1ock11111111111111111111111111111111");

const RENT_ID: Pubkey = Pubkey::from_str_const("SysvarRent111111111111111111111111111111111");

const STAKE_HISTORY_ID: Pubkey =
    Pubkey::from_str_const("SysvarStakeHistory1111111111111111111111111");

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(deprecated)]
    #[test]
    fn test_constants() {
        // Ensure that the constants are in sync with the solana program.
        assert_eq!(CLOCK_ID, solana_program::sysvar::clock::ID);

        // Ensure that the constants are in sync with the solana program.
        assert_eq!(STAKE_HISTORY_ID, solana_program::sysvar::stake_history::ID);

        // Ensure that the constants are in sync with the solana rent.
        assert_eq!(RENT_ID, solana_program::sysvar::rent::ID);
    }
}
