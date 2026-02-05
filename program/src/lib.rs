use solana_native_token::LAMPORTS_PER_SOL;

pub mod helpers;
pub mod processor;

pub mod entrypoint;

solana_pubkey::declare_id!("Stake11111111111111111111111111111111111111");

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
    const MINIMUM_DELEGATION_SOL: u64 = 1;
    MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
}
