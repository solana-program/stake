#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]
use solana_program::native_token::LAMPORTS_PER_SOL;

pub mod stake_instruction;
pub mod stake_state;

// XXX placeholder for feature_set
#[macro_export]
macro_rules! feature_set_die {
    () => {
        panic!("feature_set not supported")
    };
}

// XXX placeholders for features
const FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL: bool = false;
const FEATURE_REDUCE_STAKE_WARMUP_COOLDOWN: bool = false;
const FEATURE_STAKE_REDELEGATE_INSTRUCTION: bool = false;
const FEATURE_REQUIRE_RENT_EXEMPT_SPLIT_DESTINATION: bool = false;

/// The minimum stake amount that can be delegated, in lamports.
/// NOTE: This is also used to calculate the minimum balance of a stake account, which is the
/// rent exempt reserve _plus_ the minimum stake delegation.
#[inline(always)]
pub fn get_minimum_delegation() -> u64 {
    if FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL {
        const MINIMUM_DELEGATION_SOL: u64 = 1;
        MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
    } else {
        #[allow(deprecated)]
        solana_program::stake::MINIMUM_STAKE_DELEGATION
    }
}
