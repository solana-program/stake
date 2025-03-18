use {
    crate::{helpers::checked_add, PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH},
    solana_program::{
        clock::{Clock, Epoch},
        entrypoint::ProgramResult,
        msg,
        program_error::ProgramError,
        stake::{instruction::StakeError, stake_flags::StakeFlags, state::*},
        stake_history::StakeHistoryGetEntry,
    },
    std::convert::TryFrom,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MergeKind {
    Inactive(Meta, u64, StakeFlags),
    ActivationEpoch(Meta, Stake, StakeFlags),
    FullyActive(Meta, Stake),
}

impl MergeKind {
    pub(crate) fn meta(&self) -> &Meta {
        match self {
            Self::Inactive(meta, _, _) => meta,
            Self::ActivationEpoch(meta, _, _) => meta,
            Self::FullyActive(meta, _) => meta,
        }
    }

    pub(crate) fn active_stake(&self) -> Option<&Stake> {
        match self {
            Self::Inactive(_, _, _) => None,
            Self::ActivationEpoch(_, stake, _) => Some(stake),
            Self::FullyActive(_, stake) => Some(stake),
        }
    }

    pub(crate) fn get_if_mergeable<T: StakeHistoryGetEntry>(
        stake_state: &StakeStateV2,
        stake_lamports: u64,
        clock: &Clock,
        stake_history: &T,
    ) -> Result<Self, ProgramError> {
        match stake_state {
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                // stake must not be in a transient state. Transient here meaning
                // activating or deactivating with non-zero effective stake.
                let status = stake.delegation.stake_activating_and_deactivating(
                    clock.epoch,
                    stake_history,
                    PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
                );

                match (status.effective, status.activating, status.deactivating) {
                    (0, 0, 0) => Ok(Self::Inactive(*meta, stake_lamports, *stake_flags)),
                    (0, _, _) => Ok(Self::ActivationEpoch(*meta, *stake, *stake_flags)),
                    (_, 0, 0) => Ok(Self::FullyActive(*meta, *stake)),
                    _ => {
                        let err = StakeError::MergeTransientStake;
                        msg!("{}", err);
                        Err(err.into())
                    }
                }
            }
            StakeStateV2::Initialized(meta) => {
                Ok(Self::Inactive(*meta, stake_lamports, StakeFlags::empty()))
            }
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    pub(crate) fn metas_can_merge(stake: &Meta, source: &Meta, clock: &Clock) -> ProgramResult {
        // lockups may mismatch so long as both have expired
        let can_merge_lockups = stake.lockup == source.lockup
            || (!stake.lockup.is_in_force(clock, None) && !source.lockup.is_in_force(clock, None));
        // `rent_exempt_reserve` has no bearing on the mergeability of accounts,
        // as the source account will be culled by runtime once the operation
        // succeeds. Considering it here would needlessly prevent merging stake
        // accounts with differing data lengths, which already exist in the wild
        // due to an SDK bug
        if stake.authorized == source.authorized && can_merge_lockups {
            Ok(())
        } else {
            msg!("Unable to merge due to metadata mismatch");
            Err(StakeError::MergeMismatch.into())
        }
    }

    pub(crate) fn active_delegations_can_merge(
        stake: &Delegation,
        source: &Delegation,
    ) -> ProgramResult {
        if stake.voter_pubkey != source.voter_pubkey {
            msg!("Unable to merge due to voter mismatch");
            Err(StakeError::MergeMismatch.into())
        } else if stake.deactivation_epoch == Epoch::MAX && source.deactivation_epoch == Epoch::MAX
        {
            Ok(())
        } else {
            msg!("Unable to merge due to stake deactivation");
            Err(StakeError::MergeMismatch.into())
        }
    }

    pub(crate) fn merge(
        self,
        source: Self,
        clock: &Clock,
    ) -> Result<Option<StakeStateV2>, ProgramError> {
        Self::metas_can_merge(self.meta(), source.meta(), clock)?;
        self.active_stake()
            .zip(source.active_stake())
            .map(|(stake, source)| {
                Self::active_delegations_can_merge(&stake.delegation, &source.delegation)
            })
            .unwrap_or(Ok(()))?;
        let merged_state = match (self, source) {
            (Self::Inactive(_, _, _), Self::Inactive(_, _, _)) => None,
            (Self::Inactive(_, _, _), Self::ActivationEpoch(_, _, _)) => None,
            (
                Self::ActivationEpoch(meta, mut stake, stake_flags),
                Self::Inactive(_, source_lamports, source_stake_flags),
            ) => {
                stake.delegation.stake = checked_add(stake.delegation.stake, source_lamports)?;
                Some(StakeStateV2::Stake(
                    meta,
                    stake,
                    stake_flags.union(source_stake_flags),
                ))
            }
            (
                Self::ActivationEpoch(meta, mut stake, stake_flags),
                Self::ActivationEpoch(source_meta, source_stake, source_stake_flags),
            ) => {
                let source_lamports = checked_add(
                    source_meta.rent_exempt_reserve,
                    source_stake.delegation.stake,
                )?;
                merge_delegation_stake_and_credits_observed(
                    &mut stake,
                    source_lamports,
                    source_stake.credits_observed,
                )?;
                Some(StakeStateV2::Stake(
                    meta,
                    stake,
                    stake_flags.union(source_stake_flags),
                ))
            }
            (Self::FullyActive(meta, mut stake), Self::FullyActive(_, source_stake)) => {
                // Don't stake the source account's `rent_exempt_reserve` to
                // protect against the magic activation loophole. It will
                // instead be moved into the destination account as extra,
                // withdrawable `lamports`
                merge_delegation_stake_and_credits_observed(
                    &mut stake,
                    source_stake.delegation.stake,
                    source_stake.credits_observed,
                )?;
                Some(StakeStateV2::Stake(meta, stake, StakeFlags::empty()))
            }
            _ => return Err(StakeError::MergeMismatch.into()),
        };
        Ok(merged_state)
    }
}

pub(crate) fn merge_delegation_stake_and_credits_observed(
    stake: &mut Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> ProgramResult {
    stake.credits_observed =
        stake_weighted_credits_observed(stake, absorbed_lamports, absorbed_credits_observed)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    stake.delegation.stake = checked_add(stake.delegation.stake, absorbed_lamports)?;
    Ok(())
}

/// Calculate the effective credits observed for two stakes when merging
///
/// When merging two `ActivationEpoch` or `FullyActive` stakes, the credits
/// observed of the merged stake is the weighted average of the two stakes'
/// credits observed.
///
/// This is because we can derive the effective credits_observed by reversing
/// the staking rewards equation, _while keeping the rewards unchanged after
/// merge (i.e. strong requirement)_, like below:
///
/// a(N) => account, r => rewards, s => stake, c => credits:
/// assume:
///   a3 = merge(a1, a2)
/// then:
///   a3.s = a1.s + a2.s
///
/// Next, given:
///   aN.r = aN.c * aN.s (for every N)
/// finally:
///        a3.r = a1.r + a2.r
/// a3.c * a3.s = a1.c * a1.s + a2.c * a2.s
///        a3.c = (a1.c * a1.s + a2.c * a2.s) / (a1.s + a2.s)     // QED
///
/// (For this discussion, we omitted irrelevant variables, including distance
///  calculation against vote_account and point indirection.)
pub(crate) fn stake_weighted_credits_observed(
    stake: &Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Option<u64> {
    if stake.credits_observed == absorbed_credits_observed {
        Some(stake.credits_observed)
    } else {
        let total_stake = u128::from(stake.delegation.stake.checked_add(absorbed_lamports)?);
        let stake_weighted_credits =
            u128::from(stake.credits_observed).checked_mul(u128::from(stake.delegation.stake))?;
        let absorbed_weighted_credits =
            u128::from(absorbed_credits_observed).checked_mul(u128::from(absorbed_lamports))?;
        // Discard fractional credits as a merge side-effect friction by taking
        // the ceiling, done by adding `denominator - 1` to the numerator.
        let total_weighted_credits = stake_weighted_credits
            .checked_add(absorbed_weighted_credits)?
            .checked_add(total_stake)?
            .checked_sub(1)?;
        u64::try_from(total_weighted_credits.checked_div(total_stake)?).ok()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]

    use {
        super::*,
        crate::id,
        proptest::prelude::*,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            account_utils::StateMut,
            pubkey::Pubkey,
            stake_history::{StakeHistory, StakeHistoryEntry},
            sysvar::rent::Rent,
        },
    };

    #[test]
    fn test_things_can_merge() {
        let good_stake = Stake {
            credits_observed: 4242,
            delegation: Delegation {
                voter_pubkey: Pubkey::new_unique(),
                stake: 424242424242,
                activation_epoch: 42,
                ..Delegation::default()
            },
        };

        let identical = good_stake;
        assert!(MergeKind::active_delegations_can_merge(
            &good_stake.delegation,
            &identical.delegation
        )
        .is_ok());

        let good_delegation = good_stake.delegation;
        let different_stake_ok = Delegation {
            stake: good_delegation.stake + 1,
            ..good_delegation
        };
        assert!(
            MergeKind::active_delegations_can_merge(&good_delegation, &different_stake_ok).is_ok()
        );

        let different_activation_epoch_ok = Delegation {
            activation_epoch: good_delegation.activation_epoch + 1,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &good_delegation,
            &different_activation_epoch_ok
        )
        .is_ok());

        let bad_voter = Delegation {
            voter_pubkey: Pubkey::new_unique(),
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(&good_delegation, &bad_voter).is_err());

        let bad_deactivation_epoch = Delegation {
            deactivation_epoch: 43,
            ..good_delegation
        };
        assert!(
            MergeKind::active_delegations_can_merge(&good_delegation, &bad_deactivation_epoch)
                .is_err()
        );
        assert!(
            MergeKind::active_delegations_can_merge(&bad_deactivation_epoch, &good_delegation)
                .is_err()
        );
    }

    #[test]
    fn test_metas_can_merge() {
        // Identical Metas can merge
        assert!(
            MergeKind::metas_can_merge(&Meta::default(), &Meta::default(), &Clock::default())
                .is_ok()
        );

        let mismatched_rent_exempt_reserve_ok = Meta {
            rent_exempt_reserve: 42,
            ..Meta::default()
        };
        assert_ne!(
            mismatched_rent_exempt_reserve_ok.rent_exempt_reserve,
            Meta::default().rent_exempt_reserve,
        );
        assert!(MergeKind::metas_can_merge(
            &Meta::default(),
            &mismatched_rent_exempt_reserve_ok,
            &Clock::default()
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &mismatched_rent_exempt_reserve_ok,
            &Meta::default(),
            &Clock::default()
        )
        .is_ok());

        let mismatched_authorized_fails = Meta {
            authorized: Authorized {
                staker: Pubkey::new_unique(),
                withdrawer: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        assert_ne!(
            mismatched_authorized_fails.authorized,
            Meta::default().authorized,
        );
        assert!(MergeKind::metas_can_merge(
            &Meta::default(),
            &mismatched_authorized_fails,
            &Clock::default()
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &mismatched_authorized_fails,
            &Meta::default(),
            &Clock::default()
        )
        .is_err());

        let lockup1_timestamp = 42;
        let lockup2_timestamp = 4242;
        let lockup1_epoch = 4;
        let lockup2_epoch = 42;
        let metas_with_lockup1 = Meta {
            lockup: Lockup {
                unix_timestamp: lockup1_timestamp,
                epoch: lockup1_epoch,
                custodian: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        let metas_with_lockup2 = Meta {
            lockup: Lockup {
                unix_timestamp: lockup2_timestamp,
                epoch: lockup2_epoch,
                custodian: Pubkey::new_unique(),
            },
            ..Meta::default()
        };

        // Mismatched lockups fail when both in force
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &metas_with_lockup1,
            &metas_with_lockup2,
            &Clock::default()
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &metas_with_lockup2,
            &metas_with_lockup1,
            &Clock::default()
        )
        .is_err());

        let clock = Clock {
            epoch: lockup1_epoch + 1,
            unix_timestamp: lockup1_timestamp + 1,
            ..Clock::default()
        };

        // Mismatched lockups fail when either in force
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(
            MergeKind::metas_can_merge(&metas_with_lockup1, &metas_with_lockup2, &clock).is_err()
        );
        assert!(
            MergeKind::metas_can_merge(&metas_with_lockup2, &metas_with_lockup1, &clock).is_err()
        );

        let clock = Clock {
            epoch: lockup2_epoch + 1,
            unix_timestamp: lockup2_timestamp + 1,
            ..Clock::default()
        };

        // Mismatched lockups succeed when both expired
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(
            MergeKind::metas_can_merge(&metas_with_lockup1, &metas_with_lockup2, &clock).is_ok()
        );
        assert!(
            MergeKind::metas_can_merge(&metas_with_lockup2, &metas_with_lockup1, &clock).is_ok()
        );
    }

    #[test]
    fn test_merge_kind_get_if_mergeable() {
        let authority_pubkey = Pubkey::new_unique();
        let initial_lamports = 4242424242;
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
        let stake_lamports = rent_exempt_reserve + initial_lamports;
        let new_rate_activation_epoch = Some(0);

        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&authority_pubkey)
        };
        let mut stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .expect("stake_account");
        let mut clock = Clock::default();
        let mut stake_history = StakeHistory::default();

        // Uninitialized state fails
        assert_eq!(
            MergeKind::get_if_mergeable(
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap_err(),
            ProgramError::InvalidAccountData
        );

        // RewardsPool state fails
        stake_account.set_state(&StakeStateV2::RewardsPool).unwrap();
        assert_eq!(
            MergeKind::get_if_mergeable(
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap_err(),
            ProgramError::InvalidAccountData
        );

        // Initialized state succeeds
        stake_account
            .set_state(&StakeStateV2::Initialized(meta))
            .unwrap();
        assert_eq!(
            MergeKind::get_if_mergeable(
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::Inactive(meta, stake_lamports, StakeFlags::empty())
        );

        clock.epoch = 0;
        let mut effective = 2 * initial_lamports;
        let mut activating = 0;
        let mut deactivating = 0;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );

        clock.epoch += 1;
        activating = initial_lamports;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );

        let stake = Stake {
            delegation: Delegation {
                stake: initial_lamports,
                activation_epoch: 1,
                deactivation_epoch: 9,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        stake_account
            .set_state(&StakeStateV2::Stake(meta, stake, StakeFlags::empty()))
            .unwrap();
        // activation_epoch succeeds
        assert_eq!(
            MergeKind::get_if_mergeable(
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::ActivationEpoch(meta, stake, StakeFlags::empty()),
        );

        // all paritially activated, transient epochs fail
        loop {
            clock.epoch += 1;
            let delta = activating.min(
                (effective as f64 * warmup_cooldown_rate(clock.epoch, new_rate_activation_epoch))
                    as u64,
            );
            effective += delta;
            activating -= delta;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if activating == 0 {
                break;
            }
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &stake_account.state().unwrap(),
                    stake_account.lamports(),
                    &clock,
                    &stake_history
                )
                .unwrap_err(),
                StakeError::MergeTransientStake.into(),
            );
        }

        // all epochs for which we're fully active succeed
        while clock.epoch < stake.delegation.deactivation_epoch - 1 {
            clock.epoch += 1;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &stake_account.state().unwrap(),
                    stake_account.lamports(),
                    &clock,
                    &stake_history
                )
                .unwrap(),
                MergeKind::FullyActive(meta, stake),
            );
        }

        clock.epoch += 1;
        deactivating = stake.delegation.stake;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );
        // deactivation epoch fails, fully transient/deactivating
        assert_eq!(
            MergeKind::get_if_mergeable(
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap_err(),
            StakeError::MergeTransientStake.into(),
        );

        // all transient, deactivating epochs fail
        loop {
            clock.epoch += 1;
            let delta = deactivating.min(
                (effective as f64 * warmup_cooldown_rate(clock.epoch, new_rate_activation_epoch))
                    as u64,
            );
            effective -= delta;
            deactivating -= delta;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if deactivating == 0 {
                break;
            }
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &stake_account.state().unwrap(),
                    stake_account.lamports(),
                    &clock,
                    &stake_history
                )
                .unwrap_err(),
                StakeError::MergeTransientStake.into(),
            );
        }

        // first fully-deactivated epoch succeeds
        assert_eq!(
            MergeKind::get_if_mergeable(
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::Inactive(meta, stake_lamports, StakeFlags::empty()),
        );
    }

    #[test]
    fn test_merge_kind_merge() {
        let clock = Clock::default();
        let lamports = 424242;
        let meta = Meta {
            rent_exempt_reserve: 42,
            ..Meta::default()
        };
        let stake = Stake {
            delegation: Delegation {
                stake: 4242,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        let inactive = MergeKind::Inactive(Meta::default(), lamports, StakeFlags::empty());
        let activation_epoch = MergeKind::ActivationEpoch(meta, stake, StakeFlags::empty());
        let fully_active = MergeKind::FullyActive(meta, stake);

        assert_eq!(
            inactive.clone().merge(inactive.clone(), &clock).unwrap(),
            None
        );
        assert_eq!(
            inactive
                .clone()
                .merge(activation_epoch.clone(), &clock)
                .unwrap(),
            None
        );
        assert!(inactive
            .clone()
            .merge(fully_active.clone(), &clock)
            .is_err());
        assert!(activation_epoch
            .clone()
            .merge(fully_active.clone(), &clock)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(inactive.clone(), &clock)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(activation_epoch.clone(), &clock)
            .is_err());

        let new_state = activation_epoch
            .clone()
            .merge(inactive, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, stake.delegation.stake + lamports);

        let new_state = activation_epoch
            .clone()
            .merge(activation_epoch, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(
            delegation.stake,
            2 * stake.delegation.stake + meta.rent_exempt_reserve
        );

        let new_state = fully_active
            .clone()
            .merge(fully_active, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, 2 * stake.delegation.stake);
    }

    #[test]
    fn test_active_stake_merge() {
        let clock = Clock::default();
        let delegation_a = 4_242_424_242u64;
        let delegation_b = 6_200_000_000u64;
        let credits_a = 124_521_000u64;
        let rent_exempt_reserve = 227_000_000u64;
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::default()
        };
        let stake_a = Stake {
            delegation: Delegation {
                stake: delegation_a,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation_b,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };

        // activating stake merge, match credits observed
        let activation_epoch_a = MergeKind::ActivationEpoch(meta, stake_a, StakeFlags::empty());
        let activation_epoch_b = MergeKind::ActivationEpoch(meta, stake_b, StakeFlags::empty());
        let new_stake = activation_epoch_a
            .merge(activation_epoch_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(new_stake.credits_observed, credits_a);
        assert_eq!(
            new_stake.delegation.stake,
            delegation_a + delegation_b + rent_exempt_reserve
        );

        // active stake merge, match credits observed
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(fully_active_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(new_stake.credits_observed, credits_a);
        assert_eq!(new_stake.delegation.stake, delegation_a + delegation_b);

        // activating stake merge, unmatched credits observed
        let credits_b = 125_124_521u64;
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation_b,
                ..Delegation::default()
            },
            credits_observed: credits_b,
        };
        let activation_epoch_a = MergeKind::ActivationEpoch(meta, stake_a, StakeFlags::empty());
        let activation_epoch_b = MergeKind::ActivationEpoch(meta, stake_b, StakeFlags::empty());
        let new_stake = activation_epoch_a
            .merge(activation_epoch_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation_a + credits_b * (delegation_b + rent_exempt_reserve))
                / (delegation_a + delegation_b + rent_exempt_reserve)
                + 1
        );
        assert_eq!(
            new_stake.delegation.stake,
            delegation_a + delegation_b + rent_exempt_reserve
        );

        // active stake merge, unmatched credits observed
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(fully_active_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation_a + credits_b * delegation_b) / (delegation_a + delegation_b)
                + 1
        );
        assert_eq!(new_stake.delegation.stake, delegation_a + delegation_b);

        // active stake merge, unmatched credits observed, no need to ceiling the
        // calculation
        let delegation = 1_000_000u64;
        let credits_a = 200_000_000u64;
        let credits_b = 100_000_000u64;
        let rent_exempt_reserve = 227_000_000u64;
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::default()
        };
        let stake_a = Stake {
            delegation: Delegation {
                stake: delegation,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation,
                ..Delegation::default()
            },
            credits_observed: credits_b,
        };
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(fully_active_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation + credits_b * delegation) / (delegation + delegation)
        );
        assert_eq!(new_stake.delegation.stake, delegation * 2);
    }

    prop_compose! {
        pub fn sum_within(max: u64)(total in 1..max)
            (intermediate in 1..total, total in Just(total))
            -> (u64, u64) {
                (intermediate, total - intermediate)
        }
    }

    proptest! {
        #[test]
        fn test_stake_weighted_credits_observed(
            (credits_a, credits_b) in sum_within(u64::MAX),
            (delegation_a, delegation_b) in sum_within(u64::MAX),
        ) {
            let stake = Stake {
                delegation: Delegation {
                    stake: delegation_a,
                    ..Delegation::default()
                },
                credits_observed: credits_a
            };
            let credits_observed = stake_weighted_credits_observed(
                &stake,
                delegation_b,
                credits_b,
            ).unwrap();

            // calculated credits observed should always be between the credits of a and b
            if credits_a < credits_b {
                assert!(credits_a < credits_observed);
                assert!(credits_observed <= credits_b);
            } else {
                assert!(credits_b <= credits_observed);
                assert!(credits_observed <= credits_a);
            }

            // the difference of the combined weighted credits and the separate weighted credits
            // should be 1 or 0
            let weighted_credits_total = credits_observed as u128 * (delegation_a + delegation_b) as u128;
            let weighted_credits_a = credits_a as u128 * delegation_a as u128;
            let weighted_credits_b = credits_b as u128 * delegation_b as u128;
            let raw_diff = weighted_credits_total - (weighted_credits_a + weighted_credits_b);
            let credits_observed_diff = raw_diff / (delegation_a + delegation_b) as u128;
            assert!(credits_observed_diff <= 1);
        }
    }
}
