use {crate::stake_history::StakeHistoryEntry, solana_clock::Epoch};

pub const BASIS_POINTS_PER_UNIT: u64 = 10_000;
pub const ORIGINAL_WARMUP_COOLDOWN_RATE_BPS: u64 = 2_500; // 25%
pub const TOWER_WARMUP_COOLDOWN_RATE_BPS: u64 = 900; // 9%

#[inline]
pub fn warmup_cooldown_rate_bps(epoch: Epoch, rate_change_activation_epoch: Option<Epoch>) -> u64 {
    if rate_change_activation_epoch.is_some_and(|activation| epoch >= activation) {
        TOWER_WARMUP_COOLDOWN_RATE_BPS
    } else {
        ORIGINAL_WARMUP_COOLDOWN_RATE_BPS
    }
}

/// Calculates the potentially rate-limited stake warmup for a single account in the current epoch.
///
/// This function allocates a share of the cluster's per-epoch activation allowance
/// proportional to the account's share of the previous epoch's total activating stake.
pub fn calculate_activation_allowance(
    current_epoch: Epoch,
    account_activating_stake: u64,
    prev_epoch_cluster_state: &StakeHistoryEntry,
    rate_change_activation_epoch: Option<Epoch>,
) -> u64 {
    calculate_stake_change_allowance(
        current_epoch,
        account_activating_stake,
        prev_epoch_cluster_state.activating,
        prev_epoch_cluster_state.effective,
        rate_change_activation_epoch,
    )
}

/// Calculates the potentially rate-limited stake cooldown for a single account in the current epoch.
///
/// This function allocates a share of the cluster's per-epoch deactivation allowance
/// proportional to the account's share of the previous epoch's total deactivating stake.
pub fn calculate_deactivation_allowance(
    current_epoch: Epoch,
    account_deactivating_stake: u64,
    prev_epoch_cluster_state: &StakeHistoryEntry,
    rate_change_activation_epoch: Option<Epoch>,
) -> u64 {
    calculate_stake_change_allowance(
        current_epoch,
        account_deactivating_stake,
        prev_epoch_cluster_state.deactivating,
        prev_epoch_cluster_state.effective,
        rate_change_activation_epoch,
    )
}

/// Internal helper for the rate-limited stake change calculation.
fn calculate_stake_change_allowance(
    epoch: Epoch,
    account_portion: u64,
    cluster_portion: u64,
    cluster_effective: u64,
    rate_change_activation_epoch: Option<Epoch>,
) -> u64 {
    // Early return if there's no stake to change (also prevents divide by zero)
    if account_portion == 0 || cluster_portion == 0 || cluster_effective == 0 {
        return 0;
    }

    let rate_bps = warmup_cooldown_rate_bps(epoch, rate_change_activation_epoch);

    // Calculate this account's proportional share of the network-wide stake change allowance for the epoch.
    // Formula: `change = (account_portion / cluster_portion) * (cluster_effective * rate)`
    // Where:
    //   - `(account_portion / cluster_portion)` is this account's share of the pool.
    //   - `(cluster_effective * rate)` is the total network allowance for change this epoch.
    //
    // Re-arranged formula to maximize precision:
    // `change = (account_portion * cluster_effective * rate_bps) / (cluster_portion * BASIS_POINTS_PER_UNIT)`
    //
    // Using `u128` for the intermediate calculations to prevent overflow.
    // If the multiplication would overflow, we saturate to u128::MAX. This ensures
    // that even in extreme edge cases, the rate-limiting invariant is maintained
    // (fail-safe) rather than bypassing rate limits entirely (fail-open).
    let numerator = (account_portion as u128)
        .saturating_mul(cluster_effective as u128)
        .saturating_mul(rate_bps as u128);
    let denominator = (cluster_portion as u128).saturating_mul(BASIS_POINTS_PER_UNIT as u128);

    // Safe unwrap as denominator cannot be zero due to early return guards above
    let delta = numerator.checked_div(denominator).unwrap();
    // The calculated delta can be larger than `account_portion` if the network's stake change
    // allowance is greater than the total stake waiting to change. In this case, the account's
    // entire portion is allowed to change.
    delta.min(account_portion as u128) as u64
}

#[cfg(test)]
mod test {
    #[allow(deprecated)]
    use crate::state::{DEFAULT_WARMUP_COOLDOWN_RATE, NEW_WARMUP_COOLDOWN_RATE};
    use {
        super::*,
        crate::ulp::max_ulp_tolerance,
        proptest::prelude::*,
        test_case::{test_case, test_matrix},
    };

    #[derive(Clone, Copy, Debug)]
    enum Kind {
        Activation,
        Deactivation,
    }

    impl Kind {
        fn prev_epoch_cluster_state(
            self,
            cluster_portion: u64,
            cluster_effective: u64,
        ) -> StakeHistoryEntry {
            match self {
                Self::Activation => StakeHistoryEntry {
                    activating: cluster_portion,
                    effective: cluster_effective,
                    ..Default::default()
                },
                Self::Deactivation => StakeHistoryEntry {
                    deactivating: cluster_portion,
                    effective: cluster_effective,
                    ..Default::default()
                },
            }
        }

        fn calculate_allowance(
            self,
            current_epoch: Epoch,
            account_portion: u64,
            cluster_portion: u64,
            cluster_effective: u64,
            rate_change_activation_epoch: Option<Epoch>,
        ) -> u64 {
            let prev = self.prev_epoch_cluster_state(cluster_portion, cluster_effective);
            match self {
                Self::Activation => calculate_activation_allowance(
                    current_epoch,
                    account_portion,
                    &prev,
                    rate_change_activation_epoch,
                ),
                Self::Deactivation => calculate_deactivation_allowance(
                    current_epoch,
                    account_portion,
                    &prev,
                    rate_change_activation_epoch,
                ),
            }
        }
    }

    #[test_case(9, Some(10), ORIGINAL_WARMUP_COOLDOWN_RATE_BPS; "before activation epoch")]
    #[test_case(10, Some(10), TOWER_WARMUP_COOLDOWN_RATE_BPS; "at activation epoch")]
    #[test_case(11, Some(10), TOWER_WARMUP_COOLDOWN_RATE_BPS; "after activation epoch")]
    #[test_case(123, None, ORIGINAL_WARMUP_COOLDOWN_RATE_BPS; "without activation epoch")]
    #[test_case(0, Some(0), TOWER_WARMUP_COOLDOWN_RATE_BPS; "activation at epoch 0 uses new rate from genesis")]
    #[test_case(u64::MAX, None, ORIGINAL_WARMUP_COOLDOWN_RATE_BPS; "None never activates even at u64::MAX")]
    fn rate_bps_selects_expected(
        epoch: Epoch,
        rate_change_activation_epoch: Option<Epoch>,
        expected_bps: u64,
    ) {
        assert_eq!(
            warmup_cooldown_rate_bps(epoch, rate_change_activation_epoch),
            expected_bps
        );
    }

    #[test_matrix(
        [Kind::Activation, Kind::Deactivation],
        [(0, 1, 1), (1, 0, 1), (1, 1, 0)]
    )]
    fn zero_cases_return_zero(kind: Kind, zero_inputs: (u64, u64, u64)) {
        let (account_portion, cluster_portion, cluster_effective) = zero_inputs;
        let allowance = kind.calculate_allowance(
            0,
            account_portion,
            cluster_portion,
            cluster_effective,
            Some(0),
        );
        assert_eq!(allowance, 0);
    }

    #[test_case(
        Kind::Activation, 99, Some(100), 100, 500, 1_000, 50;
        "activation at previous rate"
    )]
    #[test_case(
        Kind::Activation, 100, Some(100), 100, 500, 1_000, 18;
        "activation at current rate"
    )]
    #[test_case(
        Kind::Deactivation, 99, Some(100), 100, 500, 1_000, 50;
        "deactivation at previous rate"
    )]
    #[test_case(
        Kind::Deactivation, 100, Some(100), 100, 500, 1_000, 18;
        "deactivation at current rate"
    )]
    fn basic_proportional_allowance_matches_expected(
        kind: Kind,
        current_epoch: Epoch,
        rate_change_activation_epoch: Option<Epoch>,
        account_portion: u64,
        cluster_portion: u64,
        cluster_effective: u64,
        expected: u64,
    ) {
        // account share = 100 / 500 -> 1/5
        // old rate: 1_000 * 25% = 250, expected 50
        // new rate: 1_000 * 9% = 90, expected 18
        let result = kind.calculate_allowance(
            current_epoch,
            account_portion,
            cluster_portion,
            cluster_effective,
            rate_change_activation_epoch,
        );
        assert_eq!(result, expected);
    }

    #[test_case(
        Kind::Activation, 99, 40, 100, 1_000_000, Some(100), 40;
        "activation caps at account portion"
    )]
    #[test_case(
        Kind::Deactivation, 0, 70, 100, 1_000_000, None, 70;
        "deactivation caps at account portion"
    )]
    fn allowance_caps_at_account_portion_when_network_allowance_is_large(
        kind: Kind,
        current_epoch: Epoch,
        account_portion: u64,
        cluster_portion: u64,
        cluster_effective: u64,
        rate_change_activation_epoch: Option<Epoch>,
        expected: u64,
    ) {
        // Total network allowance is enormous relative to waiting stake,
        // so the result should clamp to the account portion.
        let result = kind.calculate_allowance(
            current_epoch,
            account_portion,
            cluster_portion,
            cluster_effective,
            rate_change_activation_epoch,
        );
        assert_eq!(result, expected);
    }

    #[test_case(Kind::Activation)]
    #[test_case(Kind::Deactivation)]
    fn overflow_scenario_still_rate_limits(kind: Kind) {
        // Extreme scenario where a single account holding nearly the total supply
        // and tries to change everything at once. Asserting rate limiting is maintained.
        let supply_lamports: u64 = 400_000_000_000_000_000; // 400M SOL
        let account_portion = supply_lamports;

        let actual_result = kind.calculate_allowance(
            100,
            account_portion,
            supply_lamports,
            supply_lamports,
            None, // forces 25% rate
        );

        // Verify overflow actually occurs in this scenario
        let rate_bps = ORIGINAL_WARMUP_COOLDOWN_RATE_BPS;
        let would_overflow = (account_portion as u128)
            .checked_mul(supply_lamports as u128)
            .and_then(|n| n.checked_mul(rate_bps as u128))
            .is_none();
        assert!(would_overflow);

        // The ideal result (with infinite precision) is 25% of the stake.
        // 400M * 0.25 = 100M
        let ideal_allowance = supply_lamports / 4;

        // With saturation fix:
        // Numerator saturates to u128::MAX (≈ 3.4e38)
        let numerator = (account_portion as u128)
            .saturating_mul(supply_lamports as u128)
            .saturating_mul(rate_bps as u128);
        assert_eq!(numerator, u128::MAX);

        // Denominator = 4e17 * 10,000 = 4e21
        let denominator = (supply_lamports as u128).saturating_mul(BASIS_POINTS_PER_UNIT as u128);
        assert_eq!(denominator, 4_000_000_000_000_000_000_000);

        // Result = u128::MAX / 4e21 ≈ 8.5e16 (~85M SOL)
        // 85M is ~21.25% of the stake (fail-safe)
        // If we allowed unlocking the full account portion it would have been 100% (fail-open)
        let expected_result = numerator
            .checked_div(denominator)
            .unwrap()
            .min(account_portion as u128) as u64;
        assert_eq!(expected_result, 85_070_591_730_234_615);

        // Assert actual result is expected
        assert_eq!(actual_result, expected_result);
        assert!(actual_result < account_portion);
        assert!(actual_result <= ideal_allowance);
    }

    #[test]
    fn integer_division_truncation_matches_expected() {
        // Float math would yield 90.009, integer math must truncate to 90
        let account_portion = 100;
        let cluster_portion = 1000;
        let cluster_effective = 10001;
        let epoch = 20;
        let rate_change_activation_epoch = Some(10); // current 9/100

        let result = calculate_stake_change_allowance(
            epoch,
            account_portion,
            cluster_portion,
            cluster_effective,
            rate_change_activation_epoch,
        );
        assert_eq!(result, 90);
    }

    // === Legacy parity: compare the integer refactor vs legacy `f64` ===

    #[allow(deprecated)]
    fn legacy_warmup_cooldown_rate(
        current_epoch: Epoch,
        rate_change_activation_epoch: Option<Epoch>,
    ) -> f64 {
        if current_epoch < rate_change_activation_epoch.unwrap_or(u64::MAX) {
            DEFAULT_WARMUP_COOLDOWN_RATE
        } else {
            NEW_WARMUP_COOLDOWN_RATE
        }
    }

    // The original formula used prior to integer implementation
    fn calculate_stake_delta_f64_legacy(
        account_portion: u64,
        cluster_portion: u64,
        cluster_effective: u64,
        current_epoch: Epoch,
        rate_change_activation_epoch: Option<Epoch>,
    ) -> u64 {
        if cluster_portion == 0 || account_portion == 0 || cluster_effective == 0 {
            return 0;
        }
        let weight = account_portion as f64 / cluster_portion as f64;
        let rate = legacy_warmup_cooldown_rate(current_epoch, rate_change_activation_epoch);
        let newly_effective_cluster_stake = cluster_effective as f64 * rate;
        (weight * newly_effective_cluster_stake) as u64
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        #[test]
        fn rate_limited_change_consistent_with_legacy(
            account_portion in 0u64..=u64::MAX,
            cluster_portion in 0u64..=u64::MAX,
            cluster_effective in 0u64..=u64::MAX,
            current_epoch in 0u64..=2000,
            rate_change_activation_epoch_option in prop::option::of(0u64..=2000),
        ) {
            let integer_math_result = calculate_stake_change_allowance(
                current_epoch,
                account_portion,
                cluster_portion,
                cluster_effective,
                rate_change_activation_epoch_option,
            );

            let float_math_result = calculate_stake_delta_f64_legacy(
                account_portion,
                cluster_portion,
                cluster_effective,
                current_epoch,
                rate_change_activation_epoch_option,
            ).min(account_portion);

            let rate_bps =
                warmup_cooldown_rate_bps(current_epoch, rate_change_activation_epoch_option);

            // See if the u128 product would overflow: account * effective * rate_bps
            let would_overflow = (account_portion as u128)
                .checked_mul(cluster_effective as u128)
                .and_then(|n| n.checked_mul(rate_bps as u128))
                .is_none();

            if account_portion == 0 || cluster_portion == 0 || cluster_effective == 0 {
                prop_assert_eq!(integer_math_result, 0);
                prop_assert_eq!(float_math_result, 0);
            } else if would_overflow {
                // In the overflow path, the helper saturates the numerator to `u128::MAX`,
                // then divides and clamps to `account_portion`.
                let denominator = (cluster_portion as u128)
                    .checked_mul(BASIS_POINTS_PER_UNIT as u128)
                    .unwrap();
                let saturated_result = u128::MAX
                    .checked_div(denominator)
                    .unwrap()
                    .min(account_portion as u128) as u64;
                prop_assert_eq!(integer_math_result, saturated_result);
            } else {
                prop_assert!(integer_math_result <= account_portion);
                prop_assert!(float_math_result <= account_portion);

                let diff = integer_math_result.abs_diff(float_math_result);
                let tolerance = max_ulp_tolerance(integer_math_result, float_math_result);
                prop_assert!(
                    diff <= tolerance,
                    "Test failed: candidate={}, oracle={}, diff={}, tolerance={}",
                    integer_math_result, float_math_result, diff, tolerance
                );
            }
        }
    }
}
