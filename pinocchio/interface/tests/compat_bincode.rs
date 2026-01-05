#![allow(deprecated)]
#![allow(clippy::arithmetic_side_effects)]

mod common;

use {
    common::*,
    core::mem::size_of,
    p_stake_interface::state::{
        Meta, Stake, StakeStateV2, StakeStateV2Tag, StakeStateV2View, StakeStateV2ViewMut,
    },
    solana_stake_interface::{
        stake_flags::StakeFlags as OldStakeFlags,
        state::{
            Authorized as OldAuthorized, Delegation as OldDelegation, Lockup as OldLockup,
            Meta as OldMeta, Stake as OldStake, StakeStateV2 as OldStakeStateV2,
        },
    },
};

#[test]
fn given_legacy_uninitialized_bytes_when_view_then_tag_matches() {
    let old_state = legacy_uninitialized();
    let data = serialize_old(&old_state);
    assert_200_bytes(&data);
    assert_tag(&data, StakeStateV2Tag::Uninitialized);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn given_legacy_rewards_pool_bytes_when_view_then_tag_matches() {
    let old_state = legacy_rewards_pool();
    let data = serialize_old(&old_state);
    assert_200_bytes(&data);
    assert_tag(&data, StakeStateV2Tag::RewardsPool);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    assert!(matches!(view, StakeStateV2View::RewardsPool));
}

#[test]
fn given_legacy_initialized_bytes_when_view_then_fields_match() {
    let old_state = legacy_initialized();
    let data = serialize_old(&old_state);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    let OldStakeStateV2::Initialized(old_meta) = old_state else {
        panic!("expected legacy Initialized");
    };

    assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
    assert_eq!(
        to_pubkey(&meta.authorized.staker),
        old_meta.authorized.staker
    );
    assert_eq!(
        to_pubkey(&meta.authorized.withdrawer),
        old_meta.authorized.withdrawer
    );
    assert_eq!(
        meta.lockup.unix_timestamp.get(),
        old_meta.lockup.unix_timestamp
    );
    assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
    assert_eq!(to_pubkey(&meta.lockup.custodian), old_meta.lockup.custodian);
}

#[test]
fn given_legacy_stake_bytes_when_view_then_fields_match_and_reserved_preserved() {
    let old_state = legacy_stake();
    let data = serialize_old(&old_state);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { meta, stake, .. } = view else {
        panic!("expected Stake");
    };

    let OldStakeStateV2::Stake(old_meta, old_stake, _old_flags) = old_state else {
        panic!("expected legacy Stake");
    };

    assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
    assert_eq!(
        to_pubkey(&meta.authorized.staker),
        old_meta.authorized.staker
    );
    assert_eq!(
        to_pubkey(&meta.authorized.withdrawer),
        old_meta.authorized.withdrawer
    );
    assert_eq!(
        meta.lockup.unix_timestamp.get(),
        old_meta.lockup.unix_timestamp
    );
    assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
    assert_eq!(to_pubkey(&meta.lockup.custodian), old_meta.lockup.custodian);

    assert_eq!(
        to_pubkey(&stake.delegation.voter_pubkey),
        old_stake.delegation.voter_pubkey
    );
    assert_eq!(stake.delegation.stake.get(), old_stake.delegation.stake);
    assert_eq!(
        stake.delegation.activation_epoch.get(),
        old_stake.delegation.activation_epoch
    );
    assert_eq!(
        stake.delegation.deactivation_epoch.get(),
        old_stake.delegation.deactivation_epoch
    );
    assert_eq!(
        stake.delegation._reserved,
        old_stake.delegation.warmup_cooldown_rate.to_le_bytes()
    );
    assert_eq!(stake.credits_observed.get(), old_stake.credits_observed);

    assert_eq!(data[196], 1);
    assert_eq!(&data[197..200], &[0u8; 3]);
}

#[test]
fn given_legacy_bytes_when_writer_roundtrip_then_preserved() {
    let variants = [
        legacy_uninitialized(),
        legacy_initialized(),
        legacy_stake(),
        legacy_rewards_pool(),
    ];

    for old_state in variants {
        let mut data = serialize_old(&old_state);
        let expected = data.clone();

        let _writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
        assert_eq!(data, expected);
    }
}

#[test]
fn given_stake_bytes_when_view_then_borrows_expected_offsets() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 7,
        authorized: OldAuthorized {
            staker: pk([11u8; 32]),
            withdrawer: pk([22u8; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: 1,
            epoch: 2,
            custodian: pk([33u8; 32]),
        },
    };

    let old_delegation = OldDelegation {
        voter_pubkey: pk([44u8; 32]),
        stake: 123,
        activation_epoch: 9,
        deactivation_epoch: 10,
        #[allow(deprecated)]
        warmup_cooldown_rate: f64::from_le_bytes([9u8; 8]),
    };
    let old_stake = OldStake {
        delegation: old_delegation,
        credits_observed: 777,
    };

    let mut old_flags = OldStakeFlags::empty();
    #[allow(deprecated)]
    old_flags.set(OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);

    let old_state = OldStakeStateV2::Stake(old_meta, old_stake, old_flags);
    let data = serialize_old(&old_state);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { meta, stake, .. } = view else {
        panic!("expected Stake");
    };

    // Borrowed offsets:
    let meta_ptr = meta as *const Meta as *const u8;
    let stake_ptr = stake as *const Stake as *const u8;

    let expected_meta_ptr = unsafe { data.as_ptr().add(4) };
    let expected_stake_ptr = unsafe { data.as_ptr().add(4 + size_of::<Meta>()) };

    assert_eq!(meta_ptr, expected_meta_ptr);
    assert_eq!(stake_ptr, expected_stake_ptr);

    // Fields:
    assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
    assert_eq!(
        to_pubkey(&meta.authorized.staker),
        old_meta.authorized.staker
    );
    assert_eq!(
        to_pubkey(&meta.authorized.withdrawer),
        old_meta.authorized.withdrawer
    );
    assert_eq!(
        meta.lockup.unix_timestamp.get(),
        old_meta.lockup.unix_timestamp
    );
    assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
    assert_eq!(to_pubkey(&meta.lockup.custodian), old_meta.lockup.custodian);

    assert_eq!(
        to_pubkey(&stake.delegation.voter_pubkey),
        old_stake.delegation.voter_pubkey
    );
    assert_eq!(stake.delegation.stake.get(), old_stake.delegation.stake);
    assert_eq!(
        stake.delegation.activation_epoch.get(),
        old_stake.delegation.activation_epoch
    );
    assert_eq!(
        stake.delegation.deactivation_epoch.get(),
        old_stake.delegation.deactivation_epoch
    );
    assert_eq!(
        stake.delegation._reserved,
        old_stake.delegation.warmup_cooldown_rate.to_le_bytes()
    );
    assert_eq!(stake.credits_observed.get(), old_stake.credits_observed);

    // Also sanity-check the exact byte position in the raw account data.
    assert_eq!(data[196], 0b0000_0001);
    assert_eq!(&data[197..200], &[0u8; 3]);
}

#[test]
fn given_initialized_bytes_when_view_mut_then_updates_in_place() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 1,
        authorized: OldAuthorized {
            staker: pk([1u8; 32]),
            withdrawer: pk([2u8; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: 3,
            epoch: 4,
            custodian: pk([5u8; 32]),
        },
    };
    let old_state = OldStakeStateV2::Initialized(old_meta);
    let mut data = serialize_old(&old_state);

    let new_rent: u64 = 0xAABBCCDDEEFF0011;
    let mut new_cust = [9u8; 32];
    new_cust[0] = 0x42;

    // Capture pointer before mutable borrow
    let expected_ptr = unsafe { data.as_mut_ptr().add(4) };

    {
        let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
        let StakeStateV2ViewMut::Initialized(meta) = view else {
            panic!("expected Initialized");
        };

        // Prove borrow is into the original buffer.
        let meta_ptr = meta as *mut Meta as *mut u8;
        assert_eq!(meta_ptr, expected_ptr);

        meta.rent_exempt_reserve.set(new_rent);
        meta.lockup.custodian.0 = new_cust;
    }

    // Exact bytes for rent_exempt_reserve live at offset 4..12.
    assert_eq!(&data[4..12], &new_rent.to_le_bytes());

    // Check with zero-copy ref:
    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };
    assert_eq!(meta.rent_exempt_reserve.get(), new_rent);
    assert_eq!(to_pubkey(&meta.lockup.custodian), pk(new_cust));

    // Check bincode compatibility:
    let decoded = deserialize_old(&data);
    let OldStakeStateV2::Initialized(decoded_meta) = decoded else {
        panic!("expected Initialized");
    };
    assert_eq!(decoded_meta.rent_exempt_reserve, new_rent);
    assert_eq!(decoded_meta.lockup.custodian, pk(new_cust));
}

#[test]
fn given_stake_bytes_when_view_mut_then_updates_in_place() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 10,
        authorized: OldAuthorized {
            staker: pk([10u8; 32]),
            withdrawer: pk([20u8; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: -10,
            epoch: 999,
            custodian: pk([30u8; 32]),
        },
    };

    let old_delegation = OldDelegation {
        voter_pubkey: pk([40u8; 32]),
        stake: 1234,
        activation_epoch: 5,
        deactivation_epoch: 6,
        #[allow(deprecated)]
        warmup_cooldown_rate: f64::from_le_bytes([7u8; 8]),
    };
    let old_stake = OldStake {
        delegation: old_delegation,
        credits_observed: 111,
    };

    let mut old_flags = OldStakeFlags::empty();
    #[allow(deprecated)]
    old_flags.set(OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);

    let old_state = OldStakeStateV2::Stake(old_meta, old_stake, old_flags);
    let mut data = serialize_old(&old_state);

    let new_credits: u64 = 9_999_999;
    let new_stake_amt: u64 = 8_888_888;
    let _new_flags: u8 = 0;

    // Capture pointers before mutable borrow
    let expected_meta_ptr = unsafe { data.as_mut_ptr().add(4) };
    let expected_stake_ptr = unsafe { data.as_mut_ptr().add(4 + size_of::<Meta>()) };

    {
        let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
        let StakeStateV2ViewMut::Stake { meta, stake, .. } = view else {
            panic!("expected Stake");
        };

        // Borrowed offsets (no memcpy):
        let meta_ptr = meta as *mut Meta as *mut u8;
        let stake_ptr = stake as *mut Stake as *mut u8;
        assert_eq!(meta_ptr, expected_meta_ptr);
        assert_eq!(stake_ptr, expected_stake_ptr);

        // Mutate fields in place:
        stake.credits_observed.set(new_credits);
        stake.delegation.stake.set(new_stake_amt);
    }

    // flags byte is exactly at index 196
    // We cannot mutate flags via this view, so it should remain "1" (from old_flags)
    assert_eq!(data[196], 1);
    assert_eq!(&data[197..200], &[0u8; 3]);

    // Check via zero-copy ref:
    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { stake, .. } = view else {
        panic!("expected Stake");
    };
    assert_eq!(stake.credits_observed.get(), new_credits);
    assert_eq!(stake.delegation.stake.get(), new_stake_amt);

    // Check bincode compatibility:
    let decoded = deserialize_old(&data);
    let OldStakeStateV2::Stake(_m, decoded_stake, decoded_flags) = decoded else {
        panic!("expected Stake");
    };
    assert_eq!(decoded_stake.credits_observed, new_credits);
    assert_eq!(decoded_stake.delegation.stake, new_stake_amt);
    assert_eq!(decoded_flags, old_flags);
}
