use solana_native_token::LAMPORTS_PER_SOL;

pub mod helpers;
pub mod processor;

pub mod entrypoint;

solana_pubkey::declare_id!("Stake11111111111111111111111111111111111111");

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

// keccak256("CUSTOM_ERROR_INVALID_WHITELIST_PUBKEY") = 0xb526ed0b73b6db12d41d3fb3d6236f06975d57632abc3b74bd162ac5e549678d
pub const CUSTOM_ERROR_INVALID_WHITELIST_PUBKEY: u32 = 0xb526ed0b;
// keccak256("CUSTOM_ERROR_VALIDATOR_NOT_WHITELISTED") = 0x600eaac41ab02c248a21acec2a6493a36ed7391b5000148e9751c26bddeb2a9f
pub const CUSTOM_ERROR_VALIDATOR_NOT_WHITELISTED: u32 = 0x600eaac4;
// keccak256("CUSTOM_ERROR_VALIDATOR_TERM_NOT_STARTED") = 0xbede83fc8ce7d0ad6e8bf7bc79fca3005e58f3d5216cc1f4e9735bc757861af7
pub const CUSTOM_ERROR_VALIDATOR_TERM_NOT_STARTED: u32 = 0xbede83fc;
// keccak256("CUSTOM_ERROR_VALIDATOR_TERM_ENDED") = 0x9ccf307f6fe60fb8b17bb25de5d87f05c0b8b4f9b728ce50a62dc398a829f1eb
pub const CUSTOM_ERROR_VALIDATOR_TERM_ENDED: u32 = 0x9ccf307f;
