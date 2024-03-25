#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))] // XXX do i want this?
#![allow(clippy::arithmetic_side_effects)] // XXX remove?
use solana_program::native_token::LAMPORTS_PER_SOL;

// XXX split into processor, state, some other files probably
pub mod helpers;
pub mod processor;

#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;

pub use solana_program;

solana_program::declare_id!("Stake11111111111111111111111111111111111111");

// XXX placeholders for features
// we have FOUR features in the current stake program we care about:
// * stake_raise_minimum_delegation_to_1_sol / 9onWzzvCzNC2jfhxxeqRgs5q7nFAAKpCUvkj6T6GJK9i
//   this may or may not be activated by time we are done, but it should be confined to the program
//   so we use a placeholder for now to call it out but we can just change the program
// * reduce_stake_warmup_cooldown / GwtDQBghCTBgmX2cpEGNPxTEBUTQRaDMGTr5qychdGMj
//   this should be active by time we are done. this feature is in the runtime also
//   note that this is the feature that also eliminates stake config
//   we implement as if it were active
// * stake_redelegate_instruction / 2KKG3C6RBnxQo9jVVrbzsoSh41TDXLK7gBc9gduyxSzW
//   this is not active on any network and is unlikely to ever be activated
//   we do not implement it
// * require_rent_exempt_split_destination / D2aip4BBr8NPWtU9vLrwrBvbuaQ8w1zV38zFLxx4pfBV
//   this should be active by time we are done. its confined to the program
//   we use a placeholder to draw attention to it because its effects are rather tricky
const FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL: bool = false;
const FEATURE_REQUIRE_RENT_EXEMPT_SPLIT_DESTINATION: bool = false;

// feature_set::reduce_stake_warmup_cooldown changed the warmup cooldown from 25% to 9%
// a function is provided by the sdk, new_warmup_cooldown_rate_epoch(), which returns the epoch this change happened
// this function is not available to bpf programs. however, we dont need it
// the number is necessary to calculate historical effective stake from stake history
// but we only care that stake we are dealing with in the present epoch has been fully (de)activated
// this means, as long as one epoch has passed since activation where all prior stake had escaped cooldown...
// ...we can pretend the rate has always beein 9% without issue. so we do that
const PERPETUAL_NEW_WARMUP: Option<u64> = Some(1);

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
