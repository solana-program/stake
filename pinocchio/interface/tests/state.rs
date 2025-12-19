#![allow(deprecated)]
#![allow(clippy::arithmetic_side_effects)]

use {
    bincode::Options,
    core::mem::size_of,
    p_stake_interface::{
        error::StakeStateError,
        pod::{PodI64, PodPubkey, PodU32, PodU64},
        state::{MetaBytes, StakeBytes, StakeStateV2Bytes, StakeStateV2View, StakeStateV2ViewMut},
    },
    proptest::prelude::*,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as OldStakeFlags,
        state::{
            Authorized as OldAuthorized, Delegation as OldDelegation, Lockup as OldLockup,
            Meta as OldMeta, Stake as OldStake, StakeStateV2 as OldStakeStateV2,
        },
    },
    wincode::ReadError,
};

fn serialize_old(state: &OldStakeStateV2) -> Vec<u8> {
    let mut data = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .serialize(state)
        .unwrap();
    data.resize(StakeStateV2Bytes::SIZE, 0);
    data
}

fn deserialize_old(data: &[u8]) -> OldStakeStateV2 {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize::<OldStakeStateV2>(data)
        .unwrap()
}

fn pk(bytes: [u8; 32]) -> Pubkey {
    Pubkey::new_from_array(bytes)
}

#[test]
fn layout_sanity() {
    assert_eq!(StakeStateV2Bytes::SIZE, 200);
    assert_eq!(size_of::<StakeStateV2Bytes>(), 200);

    assert_eq!(size_of::<PodU32>(), 4);
    assert_eq!(size_of::<PodU64>(), 8);
    assert_eq!(size_of::<PodI64>(), 8);
    assert_eq!(size_of::<PodPubkey>(), 32);

    // Must exactly match on-chain bincode layout pieces.
    assert_eq!(size_of::<MetaBytes>(), 120);
    assert_eq!(size_of::<StakeBytes>(), 72);

    // Tag (4) + Meta (120) + Stake (72) = 196. StakeFlags lives at byte 196.
    assert_eq!(StakeStateV2Bytes::FLAGS_OFFSET_IN_PAYLOAD, 192); // within payload
}

#[test]
fn view_wrong_length_errors() {
    let data = vec![0u8; 199];
    let err = StakeStateV2View::from_account_data(&data).unwrap_err();
    assert!(matches!(
        err,
        StakeStateError::WrongLength {
            expected: 200,
            actual: 199
        }
    ));
}

#[test]
fn view_mut_wrong_length_errors() {
    let mut data = vec![0u8; 201];
    let err = StakeStateV2ViewMut::from_account_data(&mut data).unwrap_err();
    assert!(matches!(
        err,
        StakeStateError::WrongLength {
            expected: 200,
            actual: 201
        }
    ));
}

#[test]
fn view_invalid_tag_errors() {
    let mut data = [0u8; 200];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());

    let err = StakeStateV2View::from_account_data(&data).unwrap_err();
    assert!(matches!(
        err,
        StakeStateError::Read(ReadError::InvalidTagEncoding(_))
    ));
}

#[test]
fn view_mut_invalid_tag_errors() {
    let mut data = [0u8; 200];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());

    let err = StakeStateV2ViewMut::from_account_data(&mut data).unwrap_err();
    assert!(matches!(
        err,
        StakeStateError::Read(ReadError::InvalidTagEncoding(_))
    ));
}

#[test]
fn view_borrows_expected_offsets_initialized() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 0x1122334455667788,
        authorized: OldAuthorized {
            staker: pk([1u8; 32]),
            withdrawer: pk([2u8; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: -123,
            epoch: 456,
            custodian: pk([3u8; 32]),
        },
    };
    let old_state = OldStakeStateV2::Initialized(old_meta);
    let data = serialize_old(&old_state);

    let view = StakeStateV2View::from_account_data(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    // Prove it's borrowing into the original buffer (no memcpy of MetaBytes).
    let meta_ptr = meta as *const MetaBytes as *const u8;
    let expected_ptr = unsafe { data.as_ptr().add(4) };
    assert_eq!(meta_ptr, expected_ptr);

    assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
    assert_eq!(
        meta.authorized.staker.to_pubkey(),
        old_meta.authorized.staker
    );
    assert_eq!(
        meta.authorized.withdrawer.to_pubkey(),
        old_meta.authorized.withdrawer
    );
    assert_eq!(
        meta.lockup.unix_timestamp.get(),
        old_meta.lockup.unix_timestamp
    );
    assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
    assert_eq!(meta.lockup.custodian.to_pubkey(), old_meta.lockup.custodian);
}

#[test]
fn view_borrows_expected_offsets_stake() {
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

    let view = StakeStateV2View::from_account_data(&data).unwrap();
    let StakeStateV2View::Stake {
        meta,
        stake,
        stake_flags,
        ..
    } = view
    else {
        panic!("expected Stake");
    };

    // Borrowed offsets:
    let meta_ptr = meta as *const MetaBytes as *const u8;
    let stake_ptr = stake as *const StakeBytes as *const u8;

    let expected_meta_ptr = unsafe { data.as_ptr().add(4) };
    let expected_stake_ptr = unsafe { data.as_ptr().add(4 + size_of::<MetaBytes>()) };

    assert_eq!(meta_ptr, expected_meta_ptr);
    assert_eq!(stake_ptr, expected_stake_ptr);

    // Fields:
    assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
    assert_eq!(
        meta.authorized.staker.to_pubkey(),
        old_meta.authorized.staker
    );
    assert_eq!(
        meta.authorized.withdrawer.to_pubkey(),
        old_meta.authorized.withdrawer
    );
    assert_eq!(
        meta.lockup.unix_timestamp.get(),
        old_meta.lockup.unix_timestamp
    );
    assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
    assert_eq!(meta.lockup.custodian.to_pubkey(), old_meta.lockup.custodian);

    assert_eq!(
        stake.delegation.voter_pubkey.to_pubkey(),
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

    // Old stake_flags is a bit-wrapper; bincode layout is a single u8 "bits".
    assert_eq!(stake_flags, 0b0000_0001);

    // Also sanity-check the exact byte position in the raw account data.
    assert_eq!(data[196], 0b0000_0001);
    assert_eq!(&data[197..200], &[0u8; 3]);
}

#[test]
fn view_uninitialized_and_rewards_pool() {
    for (tag, expected) in [
        (StakeStateV2Bytes::TAG_UNINITIALIZED, "Uninitialized"),
        (StakeStateV2Bytes::TAG_REWARDS_POOL, "RewardsPool"),
    ] {
        let mut data = [0u8; 200];
        data[0..4].copy_from_slice(&tag.to_le_bytes());

        let view = StakeStateV2View::from_account_data(&data).unwrap();
        match (view, expected) {
            (StakeStateV2View::Uninitialized, "Uninitialized") => {}
            (StakeStateV2View::RewardsPool, "RewardsPool") => {}
            _ => panic!("unexpected variant for tag {tag}"),
        }
    }
}

#[test]
fn view_mut_initialized_updates_in_place() {
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
        let view = StakeStateV2ViewMut::from_account_data(&mut data).unwrap();
        let StakeStateV2ViewMut::Initialized { meta, .. } = view else {
            panic!("expected Initialized");
        };

        // Prove borrow is into the original buffer.
        let meta_ptr = meta as *mut MetaBytes as *mut u8;
        assert_eq!(meta_ptr, expected_ptr);

        meta.rent_exempt_reserve.set(new_rent);
        meta.lockup.custodian.0 = new_cust;
    }

    // Exact bytes for rent_exempt_reserve live at offset 4..12.
    assert_eq!(&data[4..12], &new_rent.to_le_bytes());

    // Check with zero-copy ref:
    let view = StakeStateV2View::from_account_data(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };
    assert_eq!(meta.rent_exempt_reserve.get(), new_rent);
    assert_eq!(meta.lockup.custodian.to_pubkey(), pk(new_cust));

    // Check bincode compatibility:
    let decoded = deserialize_old(&data);
    let OldStakeStateV2::Initialized(decoded_meta) = decoded else {
        panic!("expected Initialized");
    };
    assert_eq!(decoded_meta.rent_exempt_reserve, new_rent);
    assert_eq!(decoded_meta.lockup.custodian, pk(new_cust));
}

#[test]
fn view_mut_stake_updates_in_place() {
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
    let new_flags: u8 = 0;

    // Capture pointers before mutable borrow
    let expected_meta_ptr = unsafe { data.as_mut_ptr().add(4) };
    let expected_stake_ptr = unsafe { data.as_mut_ptr().add(4 + size_of::<MetaBytes>()) };

    {
        let view = StakeStateV2ViewMut::from_account_data(&mut data).unwrap();
        let StakeStateV2ViewMut::Stake {
            meta,
            stake,
            stake_flags,
            ..
        } = view
        else {
            panic!("expected Stake");
        };

        // Borrowed offsets (no memcpy):
        let meta_ptr = meta as *mut MetaBytes as *mut u8;
        let stake_ptr = stake as *mut StakeBytes as *mut u8;
        assert_eq!(meta_ptr, expected_meta_ptr);
        assert_eq!(stake_ptr, expected_stake_ptr);

        // Mutate fields in place:
        stake.credits_observed.set(new_credits);
        stake.delegation.stake.set(new_stake_amt);
        *stake_flags = new_flags;
    }

    // flags byte is exactly at index 196
    assert_eq!(data[196], new_flags);
    assert_eq!(&data[197..200], &[0u8; 3]);

    // Check via zero-copy ref:
    let view = StakeStateV2View::from_account_data(&data).unwrap();
    let StakeStateV2View::Stake {
        stake, stake_flags, ..
    } = view
    else {
        panic!("expected Stake");
    };
    assert_eq!(stake.credits_observed.get(), new_credits);
    assert_eq!(stake.delegation.stake.get(), new_stake_amt);
    assert_eq!(stake_flags, new_flags);

    // Check bincode compatibility:
    let decoded = deserialize_old(&data);
    let OldStakeStateV2::Stake(_m, decoded_stake, decoded_flags) = decoded else {
        panic!("expected Stake");
    };
    assert_eq!(decoded_stake.credits_observed, new_credits);
    assert_eq!(decoded_stake.delegation.stake, new_stake_amt);
    assert_eq!(decoded_flags, OldStakeFlags::empty());
}

proptest! {
    #[test]
    fn prop_view_matches_bincode_for_all_variants(
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
        let view = StakeStateV2View::from_account_data(&data).unwrap();

        match (variant, view) {
            (0, StakeStateV2View::Uninitialized) => {}
            (3, StakeStateV2View::RewardsPool) => {}
            (1, StakeStateV2View::Initialized(meta)) => {
                prop_assert_eq!(meta.rent_exempt_reserve.get(), rent_exempt_reserve);
                prop_assert_eq!(meta.authorized.staker.to_pubkey(), pk(staker_bytes));
                prop_assert_eq!(meta.authorized.withdrawer.to_pubkey(), pk(withdrawer_bytes));
                prop_assert_eq!(meta.lockup.unix_timestamp.get(), lockup_timestamp);
                prop_assert_eq!(meta.lockup.epoch.get(), lockup_epoch);
                prop_assert_eq!(meta.lockup.custodian.to_pubkey(), pk(custodian_bytes));
            }
            (2, StakeStateV2View::Stake { meta, stake, stake_flags, .. }) => {
                prop_assert_eq!(meta.rent_exempt_reserve.get(), rent_exempt_reserve);
                prop_assert_eq!(meta.authorized.staker.to_pubkey(), pk(staker_bytes));
                prop_assert_eq!(meta.authorized.withdrawer.to_pubkey(), pk(withdrawer_bytes));
                prop_assert_eq!(meta.lockup.unix_timestamp.get(), lockup_timestamp);
                prop_assert_eq!(meta.lockup.epoch.get(), lockup_epoch);
                prop_assert_eq!(meta.lockup.custodian.to_pubkey(), pk(custodian_bytes));

                prop_assert_eq!(stake.delegation.voter_pubkey.to_pubkey(), pk(voter_bytes));
                prop_assert_eq!(stake.delegation.stake.get(), stake_amount);
                prop_assert_eq!(stake.delegation.activation_epoch.get(), activation_epoch);
                prop_assert_eq!(stake.delegation.deactivation_epoch.get(), deactivation_epoch);
                prop_assert_eq!(stake.delegation._reserved, reserved);
                prop_assert_eq!(stake.credits_observed.get(), credits_observed);

                prop_assert_eq!(stake_flags, expected_flags_byte);
                prop_assert_eq!(data[196], expected_flags_byte);
            }
            _ => prop_assert!(false, "unexpected (variant, view) pairing"),
        }
    }

    #[test]
    fn prop_view_mut_stake_persists_updates(
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
        new_flags in any::<u8>(),
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

        let view = StakeStateV2ViewMut::from_account_data(&mut data).unwrap();
        let StakeStateV2ViewMut::Stake { meta, stake, stake_flags, .. } = view else {
            prop_assert!(false, "expected Stake");
            return Ok(());
        };

        meta.rent_exempt_reserve.set(new_rent_exempt_reserve);
        stake.credits_observed.set(new_credits_observed);
        stake.delegation.stake.set(new_stake_amount);
        *stake_flags = new_flags;

        let view = StakeStateV2View::from_account_data(&data).unwrap();
        let StakeStateV2View::Stake { meta, stake, stake_flags, .. } = view else {
            prop_assert!(false, "expected Stake");
            return Ok(());
        };

        prop_assert_eq!(meta.rent_exempt_reserve.get(), new_rent_exempt_reserve);
        prop_assert_eq!(stake.credits_observed.get(), new_credits_observed);
        prop_assert_eq!(stake.delegation.stake.get(), new_stake_amount);
        prop_assert_eq!(stake_flags, new_flags);

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
        // is the one we set (index 196) and that bincode successfully parsed.
        prop_assert_eq!(data[196], new_flags);
    }
}
