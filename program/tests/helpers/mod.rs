#![allow(clippy::arithmetic_side_effects)]
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod context;
pub mod instruction_builders;
pub mod lifecycle;
pub mod stake_tracker;
pub mod utils;

// Re-export commonly used items
pub use context::StakeTestContext;
pub use instruction_builders::{
    AuthorizeCheckedConfig, AuthorizeCheckedWithSeedConfig, AuthorizeConfig, DeactivateConfig,
    DelegateConfig, InitializeCheckedConfig, InitializeConfig, InstructionConfig,
    InstructionExecution, MergeConfig, MoveLamportsConfig, MoveLamportsFullConfig, MoveStakeConfig,
    MoveStakeWithVoteConfig, SetLockupCheckedConfig, SplitConfig, WithdrawConfig,
};
pub use lifecycle::StakeLifecycle;
pub use stake_tracker::{MolluskStakeExt, StakeTracker};
pub use utils::{
    add_sysvars, create_vote_account, get_effective_stake, increment_vote_account_credits,
    initialize_stake_account, parse_stake_account, true_up_transient_stake_epoch,
    STAKE_RENT_EXEMPTION,
};
