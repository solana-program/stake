//! Utility functions
use {crate::MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION, solana_clock::Epoch};
#[cfg(feature = "bincode")]
use {
    solana_account_info::AccountInfo,
    solana_cpi::{get_return_data, invoke_unchecked},
    solana_program_error::ProgramError,
    solana_vote_interface::state::VoteStateV4,
    std::mem::MaybeUninit,
};

/// Helper function for programs to call [`GetMinimumDelegation`] and then fetch the return data
///
/// This fn handles performing the CPI to call the [`GetMinimumDelegation`] function, and then
/// calls [`get_return_data()`] to fetch the return data.
///
/// [`GetMinimumDelegation`]: crate::instruction::StakeInstruction::GetMinimumDelegation
/// [`get_return_data()`]: solana_cpi::get_return_data
#[cfg(feature = "bincode")]
pub fn get_minimum_delegation() -> Result<u64, ProgramError> {
    let instruction = crate::instruction::get_minimum_delegation();
    invoke_unchecked(&instruction, &[])?;
    get_minimum_delegation_return_data()
}

/// Helper function for programs to get the return data after calling [`GetMinimumDelegation`]
///
/// This fn handles calling [`get_return_data()`], ensures the result is from the correct
/// program, and returns the correct type.
///
/// [`GetMinimumDelegation`]: crate::instruction::StakeInstruction::GetMinimumDelegation
/// [`get_return_data()`]: solana_cpi::get_return_data
#[cfg(feature = "bincode")]
fn get_minimum_delegation_return_data() -> Result<u64, ProgramError> {
    get_return_data()
        .ok_or(ProgramError::InvalidInstructionData)
        .and_then(|(program_id, return_data)| {
            (program_id == crate::program::id())
                .then_some(return_data)
                .ok_or(ProgramError::IncorrectProgramId)
        })
        .and_then(|return_data| {
            return_data
                .try_into()
                .or(Err(ProgramError::InvalidInstructionData))
        })
        .map(u64::from_le_bytes)
}

/// Deserialize a Vote Program-owned account as [`VoteStateV4`].
#[cfg(feature = "bincode")]
pub fn get_vote_state(vote_account_info: &AccountInfo) -> Result<Box<VoteStateV4>, ProgramError> {
    if *vote_account_info.owner != solana_vote_interface::program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let mut vote_state = Box::new(MaybeUninit::uninit());
    VoteStateV4::deserialize_into_uninit(
        &vote_account_info.try_borrow_data()?,
        vote_state.as_mut(),
        vote_account_info.key,
    )
    .map_err(|_| ProgramError::InvalidAccountData)?;
    let vote_state = unsafe { Box::from_raw(Box::into_raw(vote_state).cast::<VoteStateV4>()) };

    Ok(vote_state)
}

/// Check if the provided `epoch_credits` demonstrate active voting over the previous
/// [`MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`].
pub fn acceptable_reference_epoch_credits(
    epoch_credits: &[(Epoch, u64, u64)],
    current_epoch: Epoch,
) -> bool {
    if let Some(epoch_index) = epoch_credits
        .len()
        .checked_sub(MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION)
    {
        let mut epoch = current_epoch;
        for (vote_epoch, ..) in epoch_credits[epoch_index..].iter().rev() {
            if *vote_epoch != epoch {
                return false;
            }
            epoch = epoch.saturating_sub(1);
        }
        true
    } else {
        false
    }
}

/// Check if the provided `epoch_credits` demonstrate delinquency over the previous
/// [`MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`].
#[deprecated(
    since = "4.5.0",
    note = "Use eligible_for_deactivate_delinquent_v2 to also handle closed vote accounts"
)]
pub fn eligible_for_deactivate_delinquent(
    epoch_credits: &[(Epoch, u64, u64)],
    current_epoch: Epoch,
) -> bool {
    is_delinquent_by_epoch_credits(epoch_credits, current_epoch)
}

fn is_delinquent_by_epoch_credits(
    epoch_credits: &[(Epoch, u64, u64)],
    current_epoch: Epoch,
) -> bool {
    match epoch_credits.last() {
        None => true,
        Some((epoch, ..)) => {
            if let Some(minimum_epoch) =
                current_epoch.checked_sub(MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch)
            {
                *epoch <= minimum_epoch
            } else {
                false
            }
        }
    }
}

/// Check whether a delegated vote account is closed or meets the delinquency
/// requirement for `DeactivateDelinquent`.
///
/// Returns `true` if any of these conditions pass:
/// - The owner is not the Vote Program. Closure withdraws all lamports. If still unfunded
///   at transaction end, the account is removed and its address loads as System Program owned.
/// - Vote Program owned data starts with four zero bytes (`Uninitialized` discriminator).
///   Trailing bytes are ignored. Closure can leave zeroed data owned by the Vote Program.
///   Later instructions in the same transaction can observe it. Funding it again in that
///   transaction preserves it afterward. The address holder can also recreate and assign
///   a zeroed account without vote initialization.
/// - Vote Program owned data is shorter than four bytes and all zero, including empty data.
///   The same recreation process can allocate fewer than four bytes.
/// - The decoded vote state has no epoch credits, as when an account has never earned
///   credits or closure writes a default vote state.
/// - Its last credit epoch is at least `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`
///   epochs before `current_epoch`.
#[cfg(feature = "bincode")]
pub fn eligible_for_deactivate_delinquent_v2(
    vote_account_info: &AccountInfo,
    current_epoch: Epoch,
) -> Result<bool, ProgramError> {
    if *vote_account_info.owner != solana_vote_interface::program::id() {
        return Ok(true);
    }

    // Inspect at most enough bytes to cover the discriminator
    if vote_account_info
        .try_borrow_data()?
        .iter()
        .take(4)
        .all(|byte| *byte == 0)
    {
        return Ok(true);
    }

    let vote_state = get_vote_state(vote_account_info)?;

    Ok(is_delinquent_by_epoch_credits(
        &vote_state.epoch_credits,
        current_epoch,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acceptable_reference_epoch_credits() {
        let epoch_credits = [];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 0));

        let epoch_credits = [(0, 42, 42), (1, 42, 42), (2, 42, 42), (3, 42, 42)];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 3));

        let epoch_credits = [
            (0, 42, 42),
            (1, 42, 42),
            (2, 42, 42),
            (3, 42, 42),
            (4, 42, 42),
        ];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 3));
        assert!(acceptable_reference_epoch_credits(&epoch_credits, 4));

        let epoch_credits = [
            (1, 42, 42),
            (2, 42, 42),
            (3, 42, 42),
            (4, 42, 42),
            (5, 42, 42),
        ];
        assert!(acceptable_reference_epoch_credits(&epoch_credits, 5));

        let epoch_credits = [
            (0, 42, 42),
            (2, 42, 42),
            (3, 42, 42),
            (4, 42, 42),
            (5, 42, 42),
        ];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 5));
    }

    #[test]
    #[allow(deprecated)]
    fn test_eligible_for_deactivate_delinquent() {
        let epoch_credits = [];
        assert!(eligible_for_deactivate_delinquent(&epoch_credits, 42));

        let epoch_credits = [(0, 42, 42)];
        assert!(!eligible_for_deactivate_delinquent(&epoch_credits, 0));

        let epoch_credits = [(0, 42, 42)];
        assert!(!eligible_for_deactivate_delinquent(
            &epoch_credits,
            MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch - 1
        ));
        assert!(eligible_for_deactivate_delinquent(
            &epoch_credits,
            MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch
        ));

        let epoch_credits = [(100, 42, 42)];
        assert!(!eligible_for_deactivate_delinquent(
            &epoch_credits,
            100 + MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch - 1
        ));
        assert!(eligible_for_deactivate_delinquent(
            &epoch_credits,
            100 + MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch
        ));
    }

    #[cfg(feature = "bincode")]
    mod deactivate_delinquent_v2 {
        use {
            super::*,
            solana_account::Account,
            solana_pubkey::Pubkey,
            solana_sdk_ids::{system_program, vote},
            solana_vote_interface::state::VoteStateVersions,
            test_case::test_case,
        };

        const MINIMUM_EPOCHS: Epoch = MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch;

        fn vote_account(
            mut state: VoteStateVersions,
            epoch_credits: Vec<(Epoch, u64, u64)>,
        ) -> Account {
            match &mut state {
                VoteStateVersions::V1_14_11(state) => state.epoch_credits = epoch_credits,
                VoteStateVersions::V3(state) => state.epoch_credits = epoch_credits,
                VoteStateVersions::V4(state) => state.epoch_credits = epoch_credits,
                VoteStateVersions::Uninitialized => unreachable!(),
            }
            Account::new_data_with_space(1, &state, VoteStateV4::size_of(), &vote::id()).unwrap()
        }

        #[test_case(system_program::id(), vec![], 0; "removed_account")]
        #[test_case(Pubkey::new_unique(), vec![255], 1; "other_owner_with_invalid_data")]
        fn test_non_vote_owner(owner: Pubkey, data: Vec<u8>, lamports: u64) {
            let mut account = Account {
                owner,
                data,
                lamports,
                ..Account::default()
            };
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    0,
                ),
                Ok(true)
            );
        }

        #[test_case(vec![0; 4]; "discriminator_only")]
        #[test_case(vec![0, 0, 0, 0, 255]; "nonzero_trailing_data")]
        fn test_zero_discriminator(data: Vec<u8>) {
            let mut account = Account {
                owner: vote::id(),
                data,
                lamports: 1,
                ..Account::default()
            };
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    0,
                ),
                Ok(true)
            );
        }

        #[test_case(0; "empty")]
        #[test_case(1; "one_byte")]
        #[test_case(2; "two_bytes")]
        #[test_case(3; "three_bytes")]
        fn test_short_zero_data(len: usize) {
            let mut account = Account::new(1, len, &vote::id());
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    0,
                ),
                Ok(true)
            );
        }

        #[test_case(VoteStateVersions::V1_14_11(Box::default()); "v1_14_11")]
        #[test_case(VoteStateVersions::V3(Box::default()); "v3")]
        #[test_case(VoteStateVersions::V4(Box::default()); "v4")]
        fn test_no_epoch_credits(state: VoteStateVersions) {
            let mut account = vote_account(state, vec![]);
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    0,
                ),
                Ok(true)
            );
        }

        #[test_case(VoteStateVersions::V1_14_11(Box::default()); "v1_14_11")]
        #[test_case(VoteStateVersions::V3(Box::default()); "v3")]
        #[test_case(VoteStateVersions::V4(Box::default()); "v4")]
        fn test_recent_credits_in_each_vote_state_version(state: VoteStateVersions) {
            let mut account = vote_account(state, vec![(42, 1, 0)]);
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    42,
                ),
                Ok(false)
            );
        }

        #[test_case(0, MINIMUM_EPOCHS - 1; "before_first_possible_deactivation")]
        #[test_case(40, 40 + MINIMUM_EPOCHS - 1; "one_epoch_too_early")]
        fn test_insufficient_credit_age(last_credit_epoch: Epoch, current_epoch: Epoch) {
            let mut account = vote_account(
                VoteStateVersions::V4(Box::default()),
                vec![(last_credit_epoch, 1, 0)],
            );
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    current_epoch,
                ),
                Ok(false)
            );
        }

        #[test_case(0, MINIMUM_EPOCHS; "first_possible_deactivation")]
        #[test_case(40, 40 + MINIMUM_EPOCHS; "exact_threshold")]
        #[test_case(40, 40 + MINIMUM_EPOCHS + 1; "past_threshold")]
        fn test_sufficient_credit_age(last_credit_epoch: Epoch, current_epoch: Epoch) {
            let mut account = vote_account(
                VoteStateVersions::V4(Box::default()),
                vec![(last_credit_epoch, 1, 0)],
            );
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    current_epoch,
                ),
                Ok(true)
            );
        }

        #[test]
        fn test_latest_credit_epoch() {
            let mut account = vote_account(
                VoteStateVersions::V4(Box::default()),
                vec![(1, 1, 0), (42, 2, 1)],
            );
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    42,
                ),
                Ok(false)
            );
        }

        #[test_case(vec![0, 0, 1]; "short_nonzero_data")]
        #[test_case(vec![3, 0, 0, 0]; "truncated_v4")]
        #[test_case(vec![0, 0, 0, 1]; "nonzero_fourth_discriminator_byte")]
        fn test_invalid_vote_data(data: Vec<u8>) {
            let mut account = Account {
                owner: vote::id(),
                data,
                lamports: 1,
                ..Account::default()
            };
            assert_eq!(
                eligible_for_deactivate_delinquent_v2(
                    &AccountInfo::from((&Pubkey::new_unique(), &mut account)),
                    42,
                ),
                Err(ProgramError::InvalidAccountData)
            );
        }
    }
}
