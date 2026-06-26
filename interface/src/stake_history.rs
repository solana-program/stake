//! A type to hold data for the [`StakeHistory` sysvar][sv].
//!
//! [sv]: https://docs.anza.xyz/runtime/sysvars#stakehistory

pub use solana_stake_history::{
    Epoch, StakeHistory, StakeHistoryEntry, StakeHistoryGetEntry, MAX_ENTRIES, SIZE,
};
