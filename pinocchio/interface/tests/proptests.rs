#![allow(deprecated)]
#![allow(clippy::arithmetic_side_effects)]

mod common;
use {
    common::*,
    p_stake_interface::state::{StakeStateV2, StakeStateV2View, StakeStateV2ViewMut},
    proptest::prelude::*,
    solana_stake_interface::{
        stake_flags::StakeFlags as OldStakeFlags,
        state::{
            Authorized as OldAuthorized, Delegation as OldDelegation, Lockup as OldLockup,
            Meta as OldMeta, Stake as OldStake, StakeStateV2 as OldStakeStateV2,
        },
    },
};

proptest! {
    #[test]
    fn given_random_legacy_variants_when_view_then_matches_bincode(
        variant in 0u8..=3u8,

        rent_exempt_reserve in any::<u64>(),
        staker_bytes in any::<[u8; 32]>(),
        withdrawer_bytes in any::<[u8; 32]>(),
        lockup_timestamp in any::<i64>(),
        lockup_epoch in any::<u64>(),
        custodian_bytes in any::<[u8; 32]>(),

        voter_bytes in any::<[u8; 32]>(),
        stake_amount in any::<u64>(),
        activation_epoch in any::<u64>(),
        deactivation_epoch in any::<u64>(),
        reserved in any::<[u8; 8]>(),
        credits_observed in any::<u64>(),

        stake_flags_bits in any::<u8>(),
    ) {
        let old_meta = OldMeta {
            rent_exempt_reserve,
            authorized: OldAuthorized {
                staker: pk(staker_bytes),
                withdrawer: pk(withdrawer_bytes),
            },
            lockup: OldLockup {
                unix_timestamp: lockup_timestamp,
                epoch: lockup_epoch,
                custodian: pk(custodian_bytes),
            },
        };

        let old_delegation = OldDelegation {
            voter_pubkey: pk(voter_bytes),
            stake: stake_amount,
            activation_epoch,
            deactivation_epoch,
            #[allow(deprecated)]
            warmup_cooldown_rate: f64::from_le_bytes(reserved),
        };

        let old_stake = OldStake {
            delegation: old_delegation,
            credits_observed,
        };

        let mut old_flags = OldStakeFlags::empty();
        #[allow(deprecated)]
        if stake_flags_bits != 0 {
            old_flags.set(OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
        }
        let expected_flags_byte: u8 = if stake_flags_bits != 0 { 1 } else { 0 };

        let old_state = match variant {
            0 => OldStakeStateV2::Uninitialized,
            1 => OldStakeStateV2::Initialized(old_meta),
            2 => OldStakeStateV2::Stake(old_meta, old_stake, old_flags),
            3 => OldStakeStateV2::RewardsPool,
            _ => unreachable!(),
        };

        let data = serialize_old(&old_state);
        let view = StakeStateV2::from_bytes(&data).unwrap();

        match (variant, view) {
            (0, StakeStateV2View::Uninitialized) => {}
            (3, StakeStateV2View::RewardsPool) => {}
            (1, StakeStateV2View::Initialized(meta)) => {
                prop_assert_eq!(meta.rent_exempt_reserve.get(), rent_exempt_reserve);
                prop_assert_eq!(to_pubkey(&meta.authorized.staker), pk(staker_bytes));
                prop_assert_eq!(to_pubkey(&meta.authorized.withdrawer), pk(withdrawer_bytes));
                prop_assert_eq!(meta.lockup.unix_timestamp.get(), lockup_timestamp);
                prop_assert_eq!(meta.lockup.epoch.get(), lockup_epoch);
                prop_assert_eq!(to_pubkey(&meta.lockup.custodian), pk(custodian_bytes));
            }
            (2, StakeStateV2View::Stake { meta, stake, .. }) => {
                prop_assert_eq!(meta.rent_exempt_reserve.get(), rent_exempt_reserve);
                prop_assert_eq!(to_pubkey(&meta.authorized.staker), pk(staker_bytes));
                prop_assert_eq!(to_pubkey(&meta.authorized.withdrawer), pk(withdrawer_bytes));
                prop_assert_eq!(meta.lockup.unix_timestamp.get(), lockup_timestamp);
                prop_assert_eq!(meta.lockup.epoch.get(), lockup_epoch);
                prop_assert_eq!(to_pubkey(&meta.lockup.custodian), pk(custodian_bytes));

                prop_assert_eq!(to_pubkey(&stake.delegation.voter_pubkey), pk(voter_bytes));
                prop_assert_eq!(stake.delegation.stake.get(), stake_amount);
                prop_assert_eq!(stake.delegation.activation_epoch.get(), activation_epoch);
                prop_assert_eq!(stake.delegation.deactivation_epoch.get(), deactivation_epoch);
                prop_assert_eq!(stake.delegation._reserved, reserved);
                prop_assert_eq!(stake.credits_observed.get(), credits_observed);

                prop_assert_eq!(data[196], expected_flags_byte);
            }
            _ => prop_assert!(false, "unexpected (variant, view) pairing"),
        }
    }

    #[test]
    fn given_random_stake_when_view_mut_updates_then_persist(
        rent_exempt_reserve in any::<u64>(),
        staker_bytes in any::<[u8; 32]>(),
        withdrawer_bytes in any::<[u8; 32]>(),
        lockup_timestamp in any::<i64>(),
        lockup_epoch in any::<u64>(),
        custodian_bytes in any::<[u8; 32]>(),

        voter_bytes in any::<[u8; 32]>(),
        stake_amount in any::<u64>(),
        activation_epoch in any::<u64>(),
        deactivation_epoch in any::<u64>(),
        reserved in any::<[u8; 8]>(),
        credits_observed in any::<u64>(),

        // new values
        new_rent_exempt_reserve in any::<u64>(),
        new_credits_observed in any::<u64>(),
        new_stake_amount in any::<u64>(),

        _new_flags in any::<u8>(),
    ) {
        let old_meta = OldMeta {
            rent_exempt_reserve,
            authorized: OldAuthorized {
                staker: pk(staker_bytes),
                withdrawer: pk(withdrawer_bytes),
            },
            lockup: OldLockup {
                unix_timestamp: lockup_timestamp,
                epoch: lockup_epoch,
                custodian: pk(custodian_bytes),
            },
        };

        let old_delegation = OldDelegation {
            voter_pubkey: pk(voter_bytes),
            stake: stake_amount,
            activation_epoch,
            deactivation_epoch,
            #[allow(deprecated)]
            warmup_cooldown_rate: f64::from_le_bytes(reserved),
        };

        let old_stake = OldStake {
            delegation: old_delegation,
            credits_observed,
        };

        let old_state = OldStakeStateV2::Stake(old_meta, old_stake, OldStakeFlags::empty());
        let mut data = serialize_old(&old_state);

        let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
        let StakeStateV2ViewMut::Stake { meta, stake, .. } = view else {
            prop_assert!(false, "expected Stake");
            return Ok(());
        };

        meta.rent_exempt_reserve.set(new_rent_exempt_reserve);
        stake.credits_observed.set(new_credits_observed);
        stake.delegation.stake.set(new_stake_amount);

        let view = StakeStateV2::from_bytes(&data).unwrap();
        let StakeStateV2View::Stake { meta, stake,  .. } = view else {
            prop_assert!(false, "expected Stake");
            return Ok(());
        };

        prop_assert_eq!(meta.rent_exempt_reserve.get(), new_rent_exempt_reserve);
        prop_assert_eq!(stake.credits_observed.get(), new_credits_observed);
        prop_assert_eq!(stake.delegation.stake.get(), new_stake_amount);

        // bincode compatibility check
        let decoded = deserialize_old(&data);
        let OldStakeStateV2::Stake(decoded_meta, decoded_stake, _) = decoded else {
            prop_assert!(false, "expected Stake (bincode)");
            return Ok(());
        };
        prop_assert_eq!(decoded_meta.rent_exempt_reserve, new_rent_exempt_reserve);
        prop_assert_eq!(decoded_stake.credits_observed, new_credits_observed);
        prop_assert_eq!(decoded_stake.delegation.stake, new_stake_amount);

        // We only assert that the byte in account data
        // has NOT changed (we didn't set it) and matches initialization.
        // We set it to `expected_flags_byte` (0 or 1) based on `stake_flags_bits`.
        // The `new_flags` input is ignored.
        // We only assert that the byte in account data
        // has NOT changed (we didn't set it) and matches initialization (empty flags = 0).
        prop_assert_eq!(data[196], 0);
    }
}
