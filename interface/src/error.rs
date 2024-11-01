use {
    num_derive::{FromPrimitive, ToPrimitive},
    solana_decode_error::DecodeError,
    solana_program_error::ProgramError,
};

/// Reasons the stake might have had an error
#[derive(Debug, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum StakeError {
    // 0
    /// Not enough credits to redeem.
    NoCreditsToRedeem,

    /// Lockup has not yet expired.
    LockupInForce,

    /// Stake already deactivated.
    AlreadyDeactivated,

    /// One re-delegation permitted per epoch.
    TooSoonToRedelegate,

    /// Split amount is more than is staked.
    InsufficientStake,

    // 5
    /// Stake account with transient stake cannot be merged.
    MergeTransientStake,

    /// Stake account merge failed due to different authority, lockups or state.
    MergeMismatch,

    /// Custodian address not present.
    CustodianMissing,

    /// Custodian signature not present.
    CustodianSignatureMissing,

    /// Insufficient voting activity in the reference vote account.
    InsufficientReferenceVotes,

    // 10
    /// Stake account is not delegated to the provided vote account.
    VoteAddressMismatch,

    /// Stake account has not been delinquent for the minimum epochs required
    /// for deactivation.
    MinimumDelinquentEpochsForDeactivationNotMet,

    /// Delegation amount is less than the minimum.
    InsufficientDelegation,

    /// Stake account with transient or inactive stake cannot be redelegated.
    RedelegateTransientOrInactiveStake,

    /// Stake redelegation to the same vote account is not permitted.
    RedelegateToSameVoteAccount,

    // 15
    /// Redelegated stake must be fully activated before deactivation.
    RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted,

    /// Stake action is not permitted while the epoch rewards period is active.
    EpochRewardsActive,
}

impl std::error::Error for StakeError {}

impl core::fmt::Display for StakeError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            StakeError::NoCreditsToRedeem => {
                write!(f, "not enough credits to redeem")
            }
            StakeError::LockupInForce => {
                write!(f, "lockup has not yet expired")
            }
            StakeError::AlreadyDeactivated => {
                write!(f, "stake already deactivated")
            }
            StakeError::TooSoonToRedelegate => {
                write!(f, "one re-delegation permitted per epoch")
            }
            StakeError::InsufficientStake => {
                write!(f, "split amount is more than is staked")
            }
            StakeError::MergeTransientStake => {
                write!(f, "stake account with transient stake cannot be merged")
            }
            StakeError::MergeMismatch => {
                write!(
                    f,
                    "stake account merge failed due to different authority, lockups or state"
                )
            }
            StakeError::CustodianMissing => {
                write!(f, "custodian address not present")
            }
            StakeError::CustodianSignatureMissing => {
                write!(f, "custodian signature not present")
            }
            StakeError::InsufficientReferenceVotes => {
                write!(
                    f,
                    "insufficient voting activity in the reference vote account"
                )
            }
            StakeError::VoteAddressMismatch => {
                write!(
                    f,
                    "stake account is not delegated to the provided vote account"
                )
            }
            StakeError::MinimumDelinquentEpochsForDeactivationNotMet => {
                write!(
                    f,
                    "stake account has not been delinquent for the minimum epochs required for \
                     deactivation"
                )
            }
            StakeError::InsufficientDelegation => {
                write!(f, "delegation amount is less than the minimum")
            }
            StakeError::RedelegateTransientOrInactiveStake => {
                write!(
                    f,
                    "stake account with transient or inactive stake cannot be redelegated"
                )
            }
            StakeError::RedelegateToSameVoteAccount => {
                write!(
                    f,
                    "stake redelegation to the same vote account is not permitted"
                )
            }
            StakeError::RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted => {
                write!(
                    f,
                    "redelegated stake must be fully activated before deactivation"
                )
            }
            StakeError::EpochRewardsActive => {
                write!(
                    f,
                    "stake action is not permitted while the epoch rewards period is active"
                )
            }
        }
    }
}

impl From<StakeError> for ProgramError {
    fn from(e: StakeError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

impl<E> DecodeError<E> for StakeError {
    fn type_of() -> &'static str {
        "StakeError"
    }
}
