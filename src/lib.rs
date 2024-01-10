#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))] // XXX do i want this?
#![allow(clippy::arithmetic_side_effects)]
#![allow(dead_code)]
#![allow(unused_imports)]
use solana_program::native_token::LAMPORTS_PER_SOL;

// XXX split into processor, state, some other files probably
pub mod omnibus;

#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;

pub use solana_program;

solana_program::declare_id!("7837mbBVYX9n2m8iy2Lf2QfooTutj3WprowcsFkvLrZA");

// XXX placeholder for feature_set
#[macro_export]
macro_rules! feature_set_die {
    () => {
        panic!("feature_set not supported")
    };
}

// XXX placeholder for stake_history
#[macro_export]
macro_rules! stake_history_die {
    () => {
        panic!("stake_history not supported")
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
