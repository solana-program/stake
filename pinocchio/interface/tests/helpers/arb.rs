#![allow(deprecated)]
use {
    p_stake_interface::state::StakeStateV2Tag,
    proptest::prelude::*,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as LegacyStakeFlags,
        state::{
            Authorized as LegacyAuthorized, Delegation as LegacyDelegation, Lockup as LegacyLockup,
            Meta as LegacyMeta, Stake as LegacyStake, StakeStateV2 as LegacyStakeStateV2,
        },
    },
};

fn arb_pubkey() -> impl Strategy<Value = Pubkey> {
    any::<[u8; 32]>().prop_map(Pubkey::new_from_array)
}

prop_compose! {
    pub fn arb_legacy_meta()(
        rent in any::<u64>(),
        staker in arb_pubkey(),
        withdrawer in arb_pubkey(),
        unix in any::<i64>(),
        epoch in any::<u64>(),
        custodian in arb_pubkey(),
    ) -> LegacyMeta {
        LegacyMeta {
            rent_exempt_reserve: rent,
            authorized: LegacyAuthorized { staker, withdrawer },
            lockup: LegacyLockup { unix_timestamp: unix, epoch, custodian },
        }
    }
}

prop_compose! {
    pub fn arb_legacy_stake()(
        voter in arb_pubkey(),
        stake_amount in any::<u64>(),
        activation_epoch in any::<u64>(),
        deactivation_epoch in any::<u64>(),
        reserved_bytes in any::<[u8; 8]>(),
        credits_observed in any::<u64>(),
    ) -> LegacyStake {
        let delegation = LegacyDelegation {
            voter_pubkey: voter,
            stake: stake_amount,
            activation_epoch,
            deactivation_epoch,
            warmup_cooldown_rate: f64::from_bits(u64::from_le_bytes(reserved_bytes)),
        };
        LegacyStake { delegation, credits_observed }
    }
}

prop_compose! {
    fn arb_legacy_flags()(flag_set in any::<bool>()) -> LegacyStakeFlags {
        let mut f = LegacyStakeFlags::empty();
        if flag_set {
            f.set(LegacyStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
        }
        f
    }
}

pub fn arb_legacy_state() -> impl Strategy<Value = LegacyStakeStateV2> {
    prop_oneof![
        Just(LegacyStakeStateV2::Uninitialized),
        Just(LegacyStakeStateV2::RewardsPool),
        arb_legacy_meta().prop_map(LegacyStakeStateV2::Initialized),
        (arb_legacy_meta(), arb_legacy_stake(), arb_legacy_flags())
            .prop_map(|(m, s, f)| LegacyStakeStateV2::Stake(m, s, f)),
    ]
}

pub fn arb_valid_tag() -> impl Strategy<Value = StakeStateV2Tag> {
    prop_oneof![
        Just(StakeStateV2Tag::Uninitialized),
        Just(StakeStateV2Tag::Initialized),
        Just(StakeStateV2Tag::Stake),
        Just(StakeStateV2Tag::RewardsPool),
    ]
}
