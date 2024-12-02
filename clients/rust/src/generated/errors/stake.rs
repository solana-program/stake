//! This code was AUTOGENERATED using the codama library.
//! Please DO NOT EDIT THIS FILE, instead use visitors
//! to add features, then rerun codama to update it.
//!
//! <https://github.com/codama-idl/codama>
//!

use {num_derive::FromPrimitive, thiserror::Error};

#[derive(Clone, Debug, Eq, Error, FromPrimitive, PartialEq)]
pub enum StakeError {
    /// 0 - Not enough credits to redeem
    #[error("Not enough credits to redeem")]
    NoCreditsToRedeem = 0x0,
    /// 1 - Lockup has not yet expired
    #[error("Lockup has not yet expired")]
    LockupInForce = 0x1,
    /// 2 - Stake already deactivated
    #[error("Stake already deactivated")]
    AlreadyDeactivated = 0x2,
    /// 3 - One re-delegation permitted per epoch
    #[error("One re-delegation permitted per epoch")]
    TooSoonToRedelegate = 0x3,
    /// 4 - Split amount is more than is staked
    #[error("Split amount is more than is staked")]
    InsufficientStake = 0x4,
    /// 5 - Stake account with transient stake cannot be merged
    #[error("Stake account with transient stake cannot be merged")]
    MergeTransientStake = 0x5,
    /// 6 - Stake account merge failed due to different authority, lockups or state
    #[error("Stake account merge failed due to different authority, lockups or state")]
    MergeMismatch = 0x6,
    /// 7 - Custodian address not present
    #[error("Custodian address not present")]
    CustodianMissing = 0x7,
    /// 8 - Custodian signature not present
    #[error("Custodian signature not present")]
    CustodianSignatureMissing = 0x8,
    /// 9 - Insufficient voting activity in the reference vote account
    #[error("Insufficient voting activity in the reference vote account")]
    InsufficientReferenceVotes = 0x9,
    /// 10 - Stake account is not delegated to the provided vote account
    #[error("Stake account is not delegated to the provided vote account")]
    VoteAddressMismatch = 0xA,
    /// 11 - Stake account has not been delinquent for the minimum epochs required for deactivation
    #[error(
        "Stake account has not been delinquent for the minimum epochs required for deactivation"
    )]
    MinimumDelinquentEpochsForDeactivationNotMet = 0xB,
    /// 12 - Delegation amount is less than the minimum
    #[error("Delegation amount is less than the minimum")]
    InsufficientDelegation = 0xC,
    /// 13 - Stake account with transient or inactive stake cannot be redelegated
    #[error("Stake account with transient or inactive stake cannot be redelegated")]
    RedelegateTransientOrInactiveStake = 0xD,
    /// 14 - Stake redelegation to the same vote account is not permitted
    #[error("Stake redelegation to the same vote account is not permitted")]
    RedelegateToSameVoteAccount = 0xE,
    /// 15 - Redelegated stake must be fully activated before deactivation
    #[error("Redelegated stake must be fully activated before deactivation")]
    RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted = 0xF,
    /// 16 - Stake action is not permitted while the epoch rewards period is active
    #[error("Stake action is not permitted while the epoch rewards period is active")]
    EpochRewardsActive = 0x10,
}

impl solana_program::program_error::PrintProgramError for StakeError {
    fn print<E>(&self) {
        solana_program::msg!(&self.to_string());
    }
}
