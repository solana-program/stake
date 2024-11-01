//! The [stake native program][np].
//!
//! [np]: https://docs.solanalabs.com/runtime/sysvars#stakehistory

#[macro_use]
extern crate serde_derive;

#[cfg_attr(feature = "frozen-abi", macro_use)]
#[cfg(feature = "frozen-abi")]
extern crate solana_frozen_abi_macro;

#[allow(deprecated)]
pub mod config;
pub mod error;
pub mod instruction;
pub mod stake_flags;
pub mod stake_history;
pub mod state;
pub mod tools;

pub mod program {
    use solana_pubkey::declare_id;

    declare_id!("Stake11111111111111111111111111111111111111");
}

/// The minimum number of epochs before stake account that is delegated to a
/// delinquent vote account may be unstaked with
/// `StakeInstruction::DeactivateDelinquent`
pub const MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION: usize = 5;
