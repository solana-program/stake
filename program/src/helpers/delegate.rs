use {
    crate::PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
    solana_program::{
        account_info::AccountInfo,
        clock::Epoch,
        program_error::ProgramError,
        pubkey::Pubkey,
        stake::{
            instruction::StakeError,
            state::{Delegation, Meta, Stake},
        },
        sysvar::stake_history::StakeHistorySysvar,
        vote::state::VoteState,
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
    vote_state: &VoteState,
    activation_epoch: Epoch,
) -> Stake {
    Stake {
        delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
        credits_observed: vote_state.credits(),
    }
}

pub(crate) fn redelegate_stake(
    stake: &mut Stake,
    stake_lamports: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    epoch: Epoch,
    stake_history: &StakeHistorySysvar,
) -> Result<(), ProgramError> {
    // If stake is currently active:
    if stake.stake(
        epoch,
        stake_history,
        PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
    ) != 0
    {
        // If pubkey of new voter is the same as current,
        // and we are scheduled to start deactivating this epoch,
        // we rescind deactivation
        if stake.delegation.voter_pubkey == *voter_pubkey
            && epoch == stake.delegation.deactivation_epoch
        {
            stake.delegation.deactivation_epoch = u64::MAX;
            return Ok(());
        } else {
            // can't redelegate to another pubkey if stake is active.
            return Err(StakeError::TooSoonToRedelegate.into());
        }
    }
    // Either the stake is freshly activated, is active but has been
    // deactivated this epoch, or has fully de-activated.
    // Redelegation implies either re-activation or un-deactivation

    stake.delegation.stake = stake_lamports;
    stake.delegation.activation_epoch = epoch;
    stake.delegation.deactivation_epoch = u64::MAX;
    stake.delegation.voter_pubkey = *voter_pubkey;
    stake.credits_observed = vote_state.credits();
    Ok(())
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
