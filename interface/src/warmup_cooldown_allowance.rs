use {crate::stake_history::StakeHistoryEntry, solana_clock::Epoch};

pub const BASIS_POINTS_PER_UNIT: u64 = 10_000;
pub const ORIGINAL_WARMUP_COOLDOWN_RATE_BPS: u64 = 2_500; // 25%
pub const TOWER_WARMUP_COOLDOWN_RATE_BPS: u64 = 900; // 9%

#[inline]
pub fn warmup_cooldown_rate_bps(epoch: Epoch, new_rate_activation_epoch: Option<Epoch>) -> u64 {
    if epoch < new_rate_activation_epoch.unwrap_or(u64::MAX) {
        ORIGINAL_WARMUP_COOLDOWN_RATE_BPS
    } else {
        TOWER_WARMUP_COOLDOWN_RATE_BPS
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
    new_rate_activation_epoch: Option<Epoch>,
) -> u64 {
    rate_limited_stake_change(
        current_epoch,
        account_activating_stake,
        prev_epoch_cluster_state.activating,
        prev_epoch_cluster_state.effective,
        new_rate_activation_epoch,
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
    new_rate_activation_epoch: Option<Epoch>,
) -> u64 {
    rate_limited_stake_change(
        current_epoch,
        account_deactivating_stake,
        prev_epoch_cluster_state.deactivating,
        prev_epoch_cluster_state.effective,
        new_rate_activation_epoch,
    )
}

/// Internal helper for the rate-limited stake change calculation.
fn rate_limited_stake_change(
    epoch: Epoch,
    account_portion: u64,
    cluster_portion: u64,
    cluster_effective: u64,
    new_rate_activation_epoch: Option<Epoch>,
) -> u64 {
    // Early return if there's no stake to change (also prevents divide by zero)
    if account_portion == 0 || cluster_portion == 0 || cluster_effective == 0 {
        return 0;
    }

    let rate_bps = warmup_cooldown_rate_bps(epoch, new_rate_activation_epoch);

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
    let numerator = (account_portion as u128)
        .checked_mul(cluster_effective as u128)
        .and_then(|x| x.checked_mul(rate_bps as u128));
    let denominator = (cluster_portion as u128).saturating_mul(BASIS_POINTS_PER_UNIT as u128);

    match numerator {
        Some(n) => {
            // Safe unwrap as denominator cannot be zero
            let delta = n.checked_div(denominator).unwrap();
            // The calculated delta can be larger than `account_portion` if the network's stake change
            // allowance is greater than the total stake waiting to change. In this case, the account's
            // entire portion is allowed to change.
            delta.min(account_portion as u128) as u64
        }
        // Overflowing u128 is not a realistic scenario except in tests. However, in that case
        // it's reasonable to allow activation/deactivation of the account's entire portion.
        None => account_portion,
    }
}

#[cfg(test)]
mod test {
    #[allow(deprecated)]
    use crate::state::{DEFAULT_WARMUP_COOLDOWN_RATE, NEW_WARMUP_COOLDOWN_RATE};
    use {super::*, crate::test_utils::max_ulp_tolerance, proptest::prelude::*};

    // === Rate selector ===

    #[test]
    fn rate_bps_before_activation_epoch_uses_prev_rate() {
        let epoch = 9;
        let new_rate_activation_epoch = Some(10);
        let bps = warmup_cooldown_rate_bps(epoch, new_rate_activation_epoch);
        assert_eq!(bps, ORIGINAL_WARMUP_COOLDOWN_RATE_BPS);
    }

    #[test]
    fn rate_bps_at_or_after_activation_epoch_uses_curr_rate() {
        let epoch = 10;
        let new_rate_activation_epoch = Some(10);
        assert_eq!(
            warmup_cooldown_rate_bps(epoch, new_rate_activation_epoch),
            TOWER_WARMUP_COOLDOWN_RATE_BPS
        );
        let epoch2 = 11;
        assert_eq!(
            warmup_cooldown_rate_bps(epoch2, new_rate_activation_epoch),
            TOWER_WARMUP_COOLDOWN_RATE_BPS
        );
    }

    #[test]
    fn rate_bps_none_activation_epoch_behaves_like_prev_rate() {
        let epoch = 123;
        let bps = warmup_cooldown_rate_bps(epoch, None);
        assert_eq!(bps, ORIGINAL_WARMUP_COOLDOWN_RATE_BPS);
    }

    // === Activation allowance ===

    #[test]
    fn activation_zero_cases_return_zero() {
        // account_portion == 0
        let prev = StakeHistoryEntry {
            activating: 10,
            effective: 100,
            ..Default::default()
        };
        assert_eq!(calculate_activation_allowance(0, 0, &prev, Some(0)), 0);

        // cluster_portion == 0
        let prev = StakeHistoryEntry {
            activating: 0,
            effective: 100,
            ..Default::default()
        };
        assert_eq!(calculate_activation_allowance(0, 5, &prev, Some(0)), 0);

        // cluster_effective == 0
        let prev = StakeHistoryEntry {
            activating: 10,
            effective: 0,
            ..Default::default()
        };
        assert_eq!(calculate_activation_allowance(0, 5, &prev, Some(0)), 0);
    }

    #[test]
    fn activation_basic_proportional_prev_rate() {
        // cluster_effective = 1000, prev rate = 1/4 => total allowance = 250
        // account share = 100 / 500 -> 1/5 => expected 50
        let current_epoch = 99;
        let new_rate_activation_epoch = Some(100);
        let prev = StakeHistoryEntry {
            activating: 500,
            effective: 1000,
            ..Default::default()
        };
        let result =
            calculate_activation_allowance(current_epoch, 100, &prev, new_rate_activation_epoch);
        assert_eq!(result, 50);
    }

    #[test]
    fn activation_caps_at_account_portion_when_network_allowance_is_large() {
        // total network allowance enormous relative to waiting stake, should cap to account_portion.
        let current_epoch = 99;
        let new_rate_activation_epoch = Some(100); // prev rate (1/4)
        let prev = StakeHistoryEntry {
            activating: 100,      // cluster_portion
            effective: 1_000_000, // large cluster effective
            ..Default::default()
        };
        let account_portion = 40;
        let result = calculate_activation_allowance(
            current_epoch,
            account_portion,
            &prev,
            new_rate_activation_epoch,
        );
        assert_eq!(result, account_portion);
    }

    #[test]
    fn activation_overflow_path_returns_account_portion() {
        // Force the u128 multiply to overflow: (u64::MAX * u64::MAX * 9) overflows u128.
        // When that happens, the helper returns the full account_portion.
        let current_epoch = 0;
        let new_rate_activation_epoch = Some(0); // use "current" 9/100 to maximize multiplier
        let prev = StakeHistoryEntry {
            activating: 1,       // non-zero cluster_portion
            effective: u64::MAX, // huge cluster_effective
            ..Default::default()
        };
        let account_portion = u64::MAX;
        let result = calculate_activation_allowance(
            current_epoch,
            account_portion,
            &prev,
            new_rate_activation_epoch,
        );
        assert_eq!(result, account_portion);
    }

    // === Cooldown allowance ===

    #[test]
    fn cooldown_zero_cases_return_zero() {
        // account_portion == 0
        let prev = StakeHistoryEntry {
            deactivating: 10,
            effective: 100,
            ..Default::default()
        };
        assert_eq!(calculate_deactivation_allowance(0, 0, &prev, Some(0)), 0);

        // cluster_portion == 0
        let prev = StakeHistoryEntry {
            deactivating: 0,
            effective: 100,
            ..Default::default()
        };
        assert_eq!(calculate_deactivation_allowance(0, 5, &prev, Some(0)), 0);

        // cluster_effective == 0
        let prev = StakeHistoryEntry {
            deactivating: 10,
            effective: 0,
            ..Default::default()
        };
        assert_eq!(calculate_deactivation_allowance(0, 5, &prev, Some(0)), 0);
    }

    #[test]
    fn cooldown_basic_proportional_curr_rate() {
        // cluster_effective = 10_000, curr rate = 9/100 => total allowance = 900
        // account share = 200 / 1000 => expected 180
        let current_epoch = 5;
        let new_rate_activation_epoch = Some(5); // current (epoch >= activation)
        let prev = StakeHistoryEntry {
            deactivating: 1000,
            effective: 10_000,
            ..Default::default()
        };
        let result =
            calculate_deactivation_allowance(current_epoch, 200, &prev, new_rate_activation_epoch);
        assert_eq!(result, 180);
    }

    #[test]
    fn cooldown_caps_at_account_portion_when_network_allowance_is_large() {
        let current_epoch = 0;
        let new_rate_activation_epoch = None; // uses prev (1/4)
        let prev = StakeHistoryEntry {
            deactivating: 100,
            effective: 1_000_000,
            ..Default::default()
        };
        let account_portion = 70;
        let result = calculate_deactivation_allowance(
            current_epoch,
            account_portion,
            &prev,
            new_rate_activation_epoch,
        );
        assert_eq!(result, account_portion);
    }

    // === Symmetry & integer rounding ===

    #[test]
    fn activation_and_cooldown_are_symmetric_given_same_inputs() {
        // With equal cluster_portions and same epoch/rate, the math should match.
        let epoch = 42;
        let new_rate_activation_epoch = Some(1_000); // prev rate for both calls
        let prev = StakeHistoryEntry {
            activating: 1_000,
            deactivating: 1_000,
            effective: 5_000,
        };
        let account = 333;
        let act = calculate_activation_allowance(epoch, account, &prev, new_rate_activation_epoch);
        let cool =
            calculate_deactivation_allowance(epoch, account, &prev, new_rate_activation_epoch);
        assert_eq!(act, cool);
    }

    #[test]
    fn integer_division_truncation_matches_expected() {
        // Float math would yield 90.009, integer math must truncate to 90
        let account_portion = 100;
        let cluster_portion = 1000;
        let cluster_effective = 10001;
        let epoch = 20;
        let new_rate_activation_epoch = Some(10); // current 9/100

        let result = rate_limited_stake_change(
            epoch,
            account_portion,
            cluster_portion,
            cluster_effective,
            new_rate_activation_epoch,
        );
        assert_eq!(result, 90);
    }

    // === Property tests: compare the integer refactor vs legacy `f64` ===

    #[allow(deprecated)]
    fn legacy_warmup_cooldown_rate(
        current_epoch: Epoch,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> f64 {
        if current_epoch < new_rate_activation_epoch.unwrap_or(u64::MAX) {
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
        new_rate_activation_epoch: Option<Epoch>,
    ) -> u64 {
        if cluster_portion == 0 || account_portion == 0 || cluster_effective == 0 {
            return 0;
        }
        let weight = account_portion as f64 / cluster_portion as f64;
        let rate = legacy_warmup_cooldown_rate(current_epoch, new_rate_activation_epoch);
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
            new_rate_activation_epoch_option in prop::option::of(0u64..=2000),
        ) {
            let integer_math_result = rate_limited_stake_change(
                current_epoch,
                account_portion,
                cluster_portion,
                cluster_effective,
                new_rate_activation_epoch_option,
            );

            let float_math_result = calculate_stake_delta_f64_legacy(
                account_portion,
                cluster_portion,
                cluster_effective,
                current_epoch,
                new_rate_activation_epoch_option,
            ).min(account_portion);

            let rate_bps =
                warmup_cooldown_rate_bps(current_epoch, new_rate_activation_epoch_option);

            // See if the u128 product would overflow: account * effective * rate_bps
            let would_overflow = (account_portion as u128)
                .checked_mul(cluster_effective as u128)
                .and_then(|n| n.checked_mul(rate_bps as u128))
                .is_none();

            if account_portion == 0 || cluster_portion == 0 || cluster_effective == 0 {
                prop_assert_eq!(integer_math_result, 0);
                prop_assert_eq!(float_math_result, 0);
            } else if would_overflow {
                // In the u128 overflow region, the `f64` implementation is guaranteed to be imprecise.
                // We only assert that our implementation correctly falls back to account_portion.
                prop_assert_eq!(integer_math_result, account_portion);
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
