//! Math utilities for calculating float/int differences

/// Calculates the "Unit in the Last Place" (`ULP`) for a `u64` value, which is
/// the gap between adjacent `f64` values at that magnitude. We need this because
/// the prop test compares the integer vs float implementations. Past `2^53`, `f64`
/// can't represent every integer, so the float result can differ by a few `ULPs`
/// even when both are correct. `f64` facts:
/// - `f64` has 53 bits of precision (52 fraction bits plus an implicit leading 1).
/// - For integers `x < 2^53`, every integer is exactly representable (`ULP = 1`).
/// - At and above powers of two, spacing doubles:
///   `[2^53, 2^54) ULP = 2`
///   `[2^54, 2^55) ULP = 4`
///   `[2^55, 2^56) ULP = 8`
fn ulp_of_u64(magnitude: u64) -> u64 {
    // Avoid the special zero case by forcing at least 1
    let magnitude_f64 = magnitude.max(1) as f64;

    // spacing to the next representable f64
    let spacing = magnitude_f64.next_up() - magnitude_f64;

    // Map back to integer units, clamp so we never return 0
    spacing.max(1.0) as u64
}

/// Compute an absolute tolerance for comparing the integer result to the
/// legacy `f64`-based implementation.
///
/// Because the legacy path rounds multiple times before the final floor,
/// the integer result can differ from the float version by a small number
/// of `ULPs` ("Unit in the Last Place") even when both are "correct" for
/// their domain.
pub fn max_ulp_tolerance(candidate: u64, oracle: u64) -> u64 {
    // Measure ULP at the larger magnitude of the two results
    let mag = candidate.max(oracle);

    // Get the ULP spacing
    let ulp = ulp_of_u64(mag);

    // Use a 4x ULP tolerance to account for precision error accumulation in the
    // legacy `f64` impl:
    // - Three `u64` to `f64` conversions
    // - One division and two multiplications are rounded
    // - The `as u64` cast truncates the final `f64` result
    //
    // Proptest confirmed these can accumulate to >3 ULPs, so 4x is a safe margin.
    ulp.saturating_mul(4)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ulp_standard_calc() {
        assert_eq!(ulp_of_u64(0), 1);
        assert_eq!(ulp_of_u64(1), 1);
        assert_eq!(ulp_of_u64((1u64 << 53) - 1), 1);
        assert_eq!(ulp_of_u64(1u64 << 53), 2);
        assert_eq!(ulp_of_u64(u64::MAX), 4096);
    }

    #[test]
    fn tolerance_small_magnitudes_use_single_ulp() {
        // For magnitudes < 2^53, ULP = 1, so tolerance = 4 * 1 = 4.
        assert_eq!(max_ulp_tolerance(0, 0), 4);
        assert_eq!(max_ulp_tolerance(0, 1), 4);
        assert_eq!(max_ulp_tolerance((1u64 << 53) - 1, 1), 4);
    }

    #[test]
    fn tolerance_scales_with_magnitude_powers_of_two() {
        // Around powers of two, ULP doubles each time, so tolerance (4 * ULP) doubles.
        let below_2_53 = max_ulp_tolerance((1u64 << 53) - 1, 0); // ULP = 1
        let at_2_53 = max_ulp_tolerance(1u64 << 53, 0); // ULP = 2
        let at_2_54 = max_ulp_tolerance(1u64 << 54, 0); // ULP = 4
        let at_2_55 = max_ulp_tolerance(1u64 << 55, 0); // ULP = 8

        assert_eq!(below_2_53, 4); // 4 * 1
        assert_eq!(at_2_53, 8); // 4 * 2
        assert_eq!(at_2_54, 16); // 4 * 4
        assert_eq!(at_2_55, 32); // 4 * 8
    }

    #[test]
    fn tolerance_uses_larger_of_two_results_and_is_symmetric() {
        let small = 1u64;
        let large = 1u64 << 53; // where ULP jumps from 1 to 2

        // order of (candidate, oracle) shouldn't matter
        let ab = max_ulp_tolerance(small, large);
        let ba = max_ulp_tolerance(large, small);
        assert_eq!(ab, ba);

        // Using (large, large) should give the same tolerance, since it's based on max()
        let big_only = max_ulp_tolerance(large, large);
        assert_eq!(ab, big_only);
    }

    #[test]
    fn tolerance_at_u64_max_matches_expected_ulp() {
        // From ulp_standard_calc: ulp_of_u64(u64::MAX) == 4096
        // So tolerance = 4 * 4096 = 16384
        assert_eq!(max_ulp_tolerance(u64::MAX, 0), 4096 * 4);
        assert_eq!(max_ulp_tolerance(0, u64::MAX), 4096 * 4);
    }
}
