use solana_program::native_token::LAMPORTS_PER_SOL;

pub mod helpers;
pub mod processor;

#[cfg(feature = "bpf-entrypoint")]
pub mod entrypoint;

pub use solana_program;

solana_program::declare_id!("Stake11111111111111111111111111111111111111");

// placeholders for features
// we have ONE feature in the current stake program we care about:
// * stake_raise_minimum_delegation_to_1_sol /
//   9onWzzvCzNC2jfhxxeqRgs5q7nFAAKpCUvkj6T6GJK9i this may or may not be
//   activated by time we are done, but it should be confined to the program so
//   we use a placeholder for now to call it out. but we can just change the
//   program. it is unclear if or when it will ever be activated, because it
//   requires a validator vote
const FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL: bool = false;

// feature_set::reduce_stake_warmup_cooldown changed the warmup/cooldown from
// 25% to 9%. a function is provided by the sdk,
// new_warmup_cooldown_rate_epoch(), which returns the epoch this change
// happened. this function is not available to bpf programs. however, we dont
// need it. the number is necessary to calculate historical effective stake from
// stake history, but we only care that stake we are dealing with in the present
// epoch has been fully (de)activated. this means, as long as one epoch has
// passed since activation where all prior stake had escaped warmup/cooldown,
// we can pretend the rate has always beein 9% without issue. so we do that
const PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH: Option<u64> = Some(0);

/// The minimum stake amount that can be delegated, in lamports.
/// NOTE: This is also used to calculate the minimum balance of a delegated
/// stake account, which is the rent exempt reserve _plus_ the minimum stake
/// delegation.
#[inline(always)]
pub fn get_minimum_delegation() -> u64 {
    if FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL {
        const MINIMUM_DELEGATION_SOL: u64 = 1;
        MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
    } else {
        1
    }
}
