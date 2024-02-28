use {
    crate::helpers::TurnInto,
    solana_program::{
        account_info::AccountInfo,
        clock::Epoch,
        program_error::ProgramError,
        pubkey::Pubkey,
        stake::instruction::StakeError,
        stake::state::{Delegation, Meta, Stake},
        vote::state::VoteState,
    },
};

/// After calling `validate_delegated_amount()`, this struct contains calculated values that are used
/// by the caller.
pub(crate) struct ValidatedDelegatedInfo {
    pub stake_amount: u64,
}

pub(crate) fn new_stake(
    stake: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    activation_epoch: Epoch,
) -> Stake {
    Stake {
        delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
        credits_observed: vote_state.credits(),
    }
}

/// Ensure the stake delegation amount is valid.  This checks that the account meets the minimum
/// balance requirements of delegated stake.  If not, return an error.
pub(crate) fn validate_delegated_amount(
    account: &AccountInfo,
    meta: &Meta,
) -> Result<ValidatedDelegatedInfo, ProgramError> {
    let stake_amount = account.lamports().saturating_sub(meta.rent_exempt_reserve); // can't stake the rent

    // Stake accounts may be initialized with a stake amount below the minimum delegation so check
    // that the minimum is met before delegation.
    if stake_amount < crate::get_minimum_delegation() {
        return Err(StakeError::InsufficientDelegation.turn_into());
    }
    Ok(ValidatedDelegatedInfo { stake_amount })
}
