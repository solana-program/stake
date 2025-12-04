use {
    solana_account_info::AccountInfo,
    solana_clock::Epoch,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        error::StakeError,
        state::{Delegation, Meta, Stake},
    },
};

/// After calling `validate_delegated_amount()`, this struct contains calculated
/// values that are used by the caller.
pub(crate) struct ValidatedDelegatedInfo {
    pub stake_amount: u64,
}

pub(crate) fn new_stake(
    stake: u64,
    voter_pubkey: &Pubkey,
    credits_observed: u64,
    activation_epoch: Epoch,
) -> Stake {
    Stake {
        delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
        credits_observed,
    }
}

/// Ensure the stake delegation amount is valid.  This checks that the account
/// meets the minimum balance requirements of delegated stake.  If not, return
/// an error.
pub(crate) fn validate_delegated_amount(
    account: &AccountInfo,
    meta: &Meta,
) -> Result<ValidatedDelegatedInfo, ProgramError> {
    let stake_amount = account.lamports().saturating_sub(meta.rent_exempt_reserve); // can't stake the rent

    // Stake accounts may be initialized with a stake amount below the minimum
    // delegation so check that the minimum is met before delegation.
    if stake_amount < crate::get_minimum_delegation() {
        return Err(StakeError::InsufficientDelegation.into());
    }
    Ok(ValidatedDelegatedInfo { stake_amount })
}
