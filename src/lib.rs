#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]
use solana_program::{
    feature_set::{self, FeatureSet},
    native_token::LAMPORTS_PER_SOL,
};

pub mod stake_instruction;
pub mod stake_state;

/// The minimum stake amount that can be delegated, in lamports.
/// NOTE: This is also used to calculate the minimum balance of a stake account, which is the
/// rent exempt reserve _plus_ the minimum stake delegation.
#[inline(always)]
pub fn get_minimum_delegation(feature_set: &FeatureSet) -> u64 {
    if feature_set.is_active(&feature_set::stake_raise_minimum_delegation_to_1_sol::id()) {
        const MINIMUM_DELEGATION_SOL: u64 = 1;
        MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
    } else {
        #[allow(deprecated)]
        solana_program::stake::MINIMUM_STAKE_DELEGATION
    }
}
