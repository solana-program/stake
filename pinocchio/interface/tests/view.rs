mod common;

use {
    bincode::Options,
    common::*,
    core::mem::size_of,
    p_stake_interface::{
        error::StakeStateError,
        state::{
            Meta, Stake, StakeStateV2, StakeStateV2Layout, StakeStateV2Tag, StakeStateV2View,
            StakeStateV2ViewMut,
        },
    },
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as OldStakeFlags,
        state::{
            Authorized as OldAuthorized, Delegation as OldDelegation, Lockup as OldLockup,
            Meta as OldMeta, Stake as OldStake, StakeStateV2 as OldStakeStateV2,
        },
    },
};

const TAG_LEN: usize = StakeStateV2Tag::TAG_LEN;
const META_OFF: usize = TAG_LEN;
const STAKE_OFF: usize = TAG_LEN + size_of::<Meta>();
const FLAGS_OFF: usize = TAG_LEN + size_of::<Meta>() + size_of::<Stake>();
const PADDING_OFF: usize = FLAGS_OFF + 1;
const LAYOUT_LEN: usize = size_of::<StakeStateV2Layout>();

fn serialize_old_checked(state: &OldStakeStateV2) -> Vec<u8> {
    // Guard against silent truncation if legacy encoding ever changes.
    let mut data = bincode_opts().serialize(state).unwrap();
    assert!(
        data.len() <= LAYOUT_LEN,
        "legacy bincode encoding unexpectedly grew: len={} > {}",
        data.len(),
        LAYOUT_LEN
    );
    data.resize(LAYOUT_LEN, 0);
    data
}

fn assert_borrows_at<T>(ptr: *const T, bytes: &[u8], offset: usize) {
    let expected = unsafe { bytes.as_ptr().add(offset) };
    assert_eq!(ptr as *const u8, expected);
}

#[test]
fn given_layout_type_when_size_checked_then_200_bytes_and_offsets_match() {
    assert_eq!(LAYOUT_LEN, 200);
    let bytes = empty_state_bytes(StakeStateV2Tag::Uninitialized);
    assert_eq!(bytes.len(), size_of::<StakeStateV2Layout>());
    assert_eq!(bytes.len(), 200);

    // These computed offsets are part of the ABI contract. If these ever change,
    // we *want* a loud test failure.
    assert_eq!(TAG_LEN, 4);
    assert_eq!(META_OFF, 4);
    assert_eq!(STAKE_OFF, 4 + size_of::<Meta>());
    assert_eq!(FLAGS_OFF, 196);
    assert_eq!(PADDING_OFF, 197);
}

#[test]
fn given_empty_buffer_when_view_then_unexpected_eof() {
    let data: [u8; 0] = [];
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_tag_only_buffer_when_view_then_unexpected_eof() {
    let mut data = [0u8; TAG_LEN];
    wincode::serialize_into(&mut data.as_mut_slice(), &StakeStateV2Tag::Uninitialized).unwrap();
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_short_buffer_when_view_then_unexpected_eof() {
    let data = vec![0u8; LAYOUT_LEN - 1];
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_short_buffer_when_view_mut_then_unexpected_eof() {
    let mut data = vec![0u8; LAYOUT_LEN - 1];
    let err = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_trailing_bytes_when_view_then_ok() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.extend_from_slice(&[171; 64]); // more than 1 byte to guard against == 200 regressions
    let view = StakeStateV2::from_bytes(&data).unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn given_trailing_bytes_when_view_mut_then_ok() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.extend_from_slice(&[205; 64]);
    let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
    assert!(matches!(view, StakeStateV2ViewMut::Uninitialized));
}

#[test]
fn given_invalid_tag_when_view_then_invalid_tag() {
    let mut data = [0u8; LAYOUT_LEN];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());

    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn given_invalid_tag_when_view_mut_then_invalid_tag() {
    let mut data = [0u8; LAYOUT_LEN];
    data[0..4].copy_from_slice(&u32::MAX.to_le_bytes());

    let err = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(t) if t == u32::MAX));
}

#[test]
fn given_uninitialized_and_rewards_pool_bytes_when_view_then_variant_matches_tag() {
    for tag in [StakeStateV2Tag::Uninitialized, StakeStateV2Tag::RewardsPool] {
        let data = empty_state_bytes(tag);

        let view = StakeStateV2::from_bytes(&data).unwrap();
        match (tag, view) {
            (StakeStateV2Tag::Uninitialized, StakeStateV2View::Uninitialized) => {}
            (StakeStateV2Tag::RewardsPool, StakeStateV2View::RewardsPool) => {}
            _ => panic!("unexpected variant for tag {tag:?}"),
        }
    }
}

#[test]
fn given_uninitialized_and_rewards_pool_bytes_when_view_mut_then_variant_matches_tag() {
    for tag in [StakeStateV2Tag::Uninitialized, StakeStateV2Tag::RewardsPool] {
        let mut data = empty_state_bytes(tag);

        let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
        match (tag, view) {
            (StakeStateV2Tag::Uninitialized, StakeStateV2ViewMut::Uninitialized) => {}
            (StakeStateV2Tag::RewardsPool, StakeStateV2ViewMut::RewardsPool) => {}
            _ => panic!("unexpected view_mut variant for tag {tag:?}"),
        }
    }
}

#[test]
fn given_unaligned_slice_when_view_then_ok_for_uninitialized() {
    // Minimal unaligned smoke test for layout casting (no meta/stake access).
    let mut data = vec![0u8; LAYOUT_LEN + 1];
    let mut slice = &mut data[1..1 + TAG_LEN];
    wincode::serialize_into(&mut slice, &StakeStateV2Tag::Uninitialized).unwrap();

    let unaligned = &data[1..1 + LAYOUT_LEN];
    let view = StakeStateV2::from_bytes(unaligned).unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn given_initialized_bytes_when_view_then_borrows_expected_offsets_and_fields_match() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 1234605616436508552,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([1u8; 32]),
            withdrawer: Pubkey::new_from_array([2u8; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: -123,
            epoch: 456,
            custodian: Pubkey::new_from_array([3u8; 32]),
        },
    };
    let old_state = OldStakeStateV2::Initialized(old_meta);
    let data = serialize_old_checked(&old_state);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    // Prove it's borrowing into the original buffer (no memcpy)
    assert_borrows_at(meta as *const Meta, &data, META_OFF);

    assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
    assert_eq!(
        Pubkey::new_from_array(*meta.authorized.staker.as_bytes()),
        old_meta.authorized.staker
    );
    assert_eq!(
        Pubkey::new_from_array(*meta.authorized.withdrawer.as_bytes()),
        old_meta.authorized.withdrawer
    );
    assert_eq!(
        meta.lockup.unix_timestamp.get(),
        old_meta.lockup.unix_timestamp
    );
    assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
    assert_eq!(
        Pubkey::new_from_array(*meta.lockup.custodian.as_bytes()),
        old_meta.lockup.custodian
    );

    // Legacy-resized encoding should have zero tail for Initialized.
    assert_eq!(data[FLAGS_OFF], 0);
    assert_eq!(&data[PADDING_OFF..LAYOUT_LEN], &[0u8; 3]);
}

#[test]
fn given_initialized_bytes_on_unaligned_slice_when_view_then_borrows_and_fields_match() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 42,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([17; 32]),
            withdrawer: Pubkey::new_from_array([34; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: i64::MIN + 7,
            epoch: u64::MAX - 9,
            custodian: Pubkey::new_from_array([51; 32]),
        },
    };
    let old_state = OldStakeStateV2::Initialized(old_meta);
    let aligned = serialize_old_checked(&old_state);

    let mut backing = vec![0u8; LAYOUT_LEN + 1];
    backing[1..1 + LAYOUT_LEN].copy_from_slice(&aligned);
    let unaligned = &backing[1..1 + LAYOUT_LEN];

    let view = StakeStateV2::from_bytes(unaligned).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    assert_borrows_at(meta as *const Meta, unaligned, META_OFF);

    assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
    assert_eq!(
        Pubkey::new_from_array(*meta.authorized.staker.as_bytes()),
        old_meta.authorized.staker
    );
    assert_eq!(
        Pubkey::new_from_array(*meta.authorized.withdrawer.as_bytes()),
        old_meta.authorized.withdrawer
    );
    assert_eq!(
        meta.lockup.unix_timestamp.get(),
        old_meta.lockup.unix_timestamp
    );
    assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
    assert_eq!(
        Pubkey::new_from_array(*meta.lockup.custodian.as_bytes()),
        old_meta.lockup.custodian
    );
}

#[test]
fn given_stake_bytes_when_view_then_borrows_expected_offsets_and_fields_match_and_legacy_bits_preserved(
) {
    let old_meta = OldMeta {
        rent_exempt_reserve: 1,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([68; 32]),
            withdrawer: Pubkey::new_from_array([85; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: -1,
            epoch: 1,
            custodian: Pubkey::new_from_array([102; 32]),
        },
    };

    let reserved_bytes = [170u8; 8];
    #[allow(deprecated)]
    let old_delegation = OldDelegation {
        voter_pubkey: Pubkey::new_from_array([119; 32]),
        stake: u64::MAX,
        activation_epoch: 0,
        deactivation_epoch: u64::MAX,
        warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
    };

    let old_stake = OldStake {
        delegation: old_delegation,
        credits_observed: u64::MAX - 1,
    };

    let mut old_flags = OldStakeFlags::empty();
    #[allow(deprecated)]
    old_flags.set(OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);

    // Serialize old_flags to get the raw bits byte (bits is private)
    let expected_flags_byte = bincode_opts().serialize(&old_flags).unwrap()[0];

    let old_state = OldStakeStateV2::Stake(old_meta, old_stake, old_flags);
    let data = serialize_old_checked(&old_state);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };

    // Prove it's borrowing into the original buffer (no memcpy)
    assert_borrows_at(meta as *const Meta, &data, META_OFF);
    assert_borrows_at(stake as *const Stake, &data, STAKE_OFF);

    // Meta fields
    assert_eq!(meta.rent_exempt_reserve.get(), 1);
    assert_eq!(
        Pubkey::new_from_array(*meta.authorized.staker.as_bytes()),
        Pubkey::new_from_array([68; 32])
    );
    assert_eq!(
        Pubkey::new_from_array(*meta.authorized.withdrawer.as_bytes()),
        Pubkey::new_from_array([85; 32])
    );
    assert_eq!(meta.lockup.unix_timestamp.get(), -1);
    assert_eq!(meta.lockup.epoch.get(), 1);
    assert_eq!(
        Pubkey::new_from_array(*meta.lockup.custodian.as_bytes()),
        Pubkey::new_from_array([102; 32])
    );

    // Stake fields
    assert_eq!(
        Pubkey::new_from_array(*stake.delegation.voter_pubkey.as_bytes()),
        Pubkey::new_from_array([119; 32])
    );
    assert_eq!(stake.delegation.stake.get(), u64::MAX);
    assert_eq!(stake.delegation.activation_epoch.get(), 0);
    assert_eq!(stake.delegation.deactivation_epoch.get(), u64::MAX);
    assert_eq!(
        stake.delegation._reserved, reserved_bytes,
        "legacy warmup_cooldown_rate bytes must land in _reserved exactly"
    );
    assert_eq!(stake.credits_observed.get(), u64::MAX - 1);

    // Raw flags byte must sit at the known offset.
    assert_eq!(data[FLAGS_OFF], expected_flags_byte);
}

#[test]
fn given_stake_bytes_on_unaligned_slice_when_view_then_borrows_and_fields_match() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 9,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([1; 32]),
            withdrawer: Pubkey::new_from_array([2; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: 3,
            epoch: 4,
            custodian: Pubkey::new_from_array([5; 32]),
        },
    };

    let reserved_bytes = [94u8; 8];
    #[allow(deprecated)]
    let old_delegation = OldDelegation {
        voter_pubkey: Pubkey::new_from_array([6; 32]),
        stake: 7,
        activation_epoch: 8,
        deactivation_epoch: 9,
        warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
    };
    let old_stake = OldStake {
        delegation: old_delegation,
        credits_observed: 10,
    };

    let old_state = OldStakeStateV2::Stake(old_meta, old_stake, OldStakeFlags::empty());
    let aligned = serialize_old_checked(&old_state);

    let mut backing = vec![0u8; LAYOUT_LEN + 1];
    backing[1..1 + LAYOUT_LEN].copy_from_slice(&aligned);
    let unaligned = &backing[1..1 + LAYOUT_LEN];

    let view = StakeStateV2::from_bytes(unaligned).unwrap();
    let StakeStateV2View::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };

    assert_borrows_at(meta as *const Meta, unaligned, META_OFF);
    assert_borrows_at(stake as *const Stake, unaligned, STAKE_OFF);

    assert_eq!(meta.rent_exempt_reserve.get(), 9);
    assert_eq!(
        Pubkey::new_from_array(*stake.delegation.voter_pubkey.as_bytes()),
        Pubkey::new_from_array([6; 32])
    );
    assert_eq!(stake.delegation._reserved, reserved_bytes);
    assert_eq!(stake.credits_observed.get(), 10);
}

#[test]
fn given_initialized_bytes_when_view_mut_then_updates_in_place_preserves_tail_and_remains_legacy_compatible(
) {
    let old_meta = OldMeta {
        rent_exempt_reserve: 1,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([1u8; 32]),
            withdrawer: Pubkey::new_from_array([2u8; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: 3,
            epoch: 4,
            custodian: Pubkey::new_from_array([5u8; 32]),
        },
    };
    let old_state = OldStakeStateV2::Initialized(old_meta);
    let mut data = serialize_old_checked(&old_state);

    // Overwrite tail to prove view_mut does not clobber unknown tail bytes.
    data[FLAGS_OFF] = 222;
    data[PADDING_OFF..LAYOUT_LEN].copy_from_slice(&[173, 190, 239]);
    let tail_before = data[FLAGS_OFF..LAYOUT_LEN].to_vec();

    let new_rent: u64 = 12302652060662169617;
    let mut new_cust = [9u8; 32];
    new_cust[0] = 66;

    // Capture pointer before mutable borrow for offset verification
    let expected_meta_ptr = unsafe { data.as_mut_ptr().add(META_OFF) };

    {
        let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
        let StakeStateV2ViewMut::Initialized(meta) = view else {
            panic!("expected Initialized");
        };

        // Verify zero-copy: reference points into original buffer at offset META_OFF
        let meta_ptr = meta as *mut Meta as *mut u8;
        assert_eq!(meta_ptr, expected_meta_ptr);

        // Mutate in place
        meta.rent_exempt_reserve.set(new_rent);
        meta.lockup.custodian.0 = new_cust;
    }

    // Verify exact bytes for rent_exempt_reserve at META_OFF..META_OFF+8
    assert_eq!(&data[META_OFF..META_OFF + 8], &new_rent.to_le_bytes());

    // Tail must be preserved byte-for-byte.
    assert_eq!(
        &data[FLAGS_OFF..LAYOUT_LEN],
        &tail_before[..],
        "view_mut(Initialized) must not modify tail bytes"
    );

    // Verify via zero-copy view
    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };
    assert_eq!(meta.rent_exempt_reserve.get(), new_rent);
    assert_eq!(
        Pubkey::new_from_array(*meta.lockup.custodian.as_bytes()),
        Pubkey::new_from_array(new_cust)
    );

    // Verify legacy bincode compatibility
    let decoded = deserialize_old(&data);
    let OldStakeStateV2::Initialized(decoded_meta) = decoded else {
        panic!("expected Initialized");
    };
    assert_eq!(decoded_meta.rent_exempt_reserve, new_rent);
    assert_eq!(
        decoded_meta.lockup.custodian,
        Pubkey::new_from_array(new_cust)
    );
}

#[test]
fn given_stake_bytes_when_view_mut_then_updates_in_place_preserves_reserved_and_tail_and_remains_legacy_compatible(
) {
    let old_meta = OldMeta {
        rent_exempt_reserve: 111,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([17; 32]),
            withdrawer: Pubkey::new_from_array([34; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: -7,
            epoch: 9,
            custodian: Pubkey::new_from_array([51; 32]),
        },
    };

    let reserved_bytes = [165u8; 8];
    #[allow(deprecated)]
    let old_delegation = OldDelegation {
        voter_pubkey: Pubkey::new_from_array([68; 32]),
        stake: 555,
        activation_epoch: 1,
        deactivation_epoch: 2,
        warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
    };
    let old_stake = OldStake {
        delegation: old_delegation,
        credits_observed: 777,
    };

    let old_state = OldStakeStateV2::Stake(old_meta, old_stake, OldStakeFlags::empty());
    let mut data = serialize_old_checked(&old_state);

    // Overwrite tail to arbitrary values and prove we preserve them.
    data[FLAGS_OFF] = 90;
    data[PADDING_OFF..LAYOUT_LEN].copy_from_slice(&[222, 173, 190]);
    let tail_before = data[FLAGS_OFF..LAYOUT_LEN].to_vec();

    // Mutations we perform via view_mut
    let new_rent = 999u64;
    let new_credits = 888u64;
    let new_stake_amount = 777u64;

    // Capture expected pointers relative to the underlying buffer
    let expected_meta_ptr = unsafe { data.as_mut_ptr().add(META_OFF) };
    let expected_stake_ptr = unsafe { data.as_mut_ptr().add(STAKE_OFF) };

    {
        let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
        let StakeStateV2ViewMut::Stake { meta, stake } = view else {
            panic!("expected Stake");
        };

        assert_eq!(meta as *mut Meta as *mut u8, expected_meta_ptr);
        assert_eq!(stake as *mut Stake as *mut u8, expected_stake_ptr);

        meta.rent_exempt_reserve.set(new_rent);
        stake.credits_observed.set(new_credits);
        stake.delegation.stake.set(new_stake_amount);

        // Reserved bytes must never be clobbered by adjacent writes.
        assert_eq!(
            stake.delegation._reserved, reserved_bytes,
            "reserved delegation bytes must remain unchanged during stake mutations"
        );
    }

    // Tail must be preserved byte-for-byte.
    assert_eq!(
        &data[FLAGS_OFF..LAYOUT_LEN],
        &tail_before[..],
        "view_mut(Stake) must not modify tail bytes"
    );

    // Verify via zero-copy view
    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };
    assert_eq!(meta.rent_exempt_reserve.get(), new_rent);
    assert_eq!(stake.credits_observed.get(), new_credits);
    assert_eq!(stake.delegation.stake.get(), new_stake_amount);
    assert_eq!(stake.delegation._reserved, reserved_bytes);

    // Verify legacy bincode decode still works and fields match.
    let decoded = deserialize_old(&data);
    let OldStakeStateV2::Stake(decoded_meta, decoded_stake, _decoded_flags) = decoded else {
        panic!("expected legacy Stake");
    };

    assert_eq!(decoded_meta.rent_exempt_reserve, new_rent);
    assert_eq!(decoded_stake.credits_observed, new_credits);
    assert_eq!(decoded_stake.delegation.stake, new_stake_amount);

    // Warmup cooldown rate bytes must be preserved exactly.
    #[allow(deprecated)]
    let decoded_reserved = decoded_stake.delegation.warmup_cooldown_rate.to_le_bytes();
    assert_eq!(decoded_reserved, reserved_bytes);
}

#[test]
fn given_stake_bytes_on_unaligned_slice_when_view_mut_then_updates_in_place_and_preserves_tail() {
    let old_meta = OldMeta {
        rent_exempt_reserve: 1,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([1; 32]),
            withdrawer: Pubkey::new_from_array([2; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: 3,
            epoch: 4,
            custodian: Pubkey::new_from_array([5; 32]),
        },
    };

    let reserved_bytes = [19u8; 8];
    #[allow(deprecated)]
    let old_delegation = OldDelegation {
        voter_pubkey: Pubkey::new_from_array([6; 32]),
        stake: 7,
        activation_epoch: 8,
        deactivation_epoch: 9,
        warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
    };
    let old_stake = OldStake {
        delegation: old_delegation,
        credits_observed: 10,
    };

    let old_state = OldStakeStateV2::Stake(old_meta, old_stake, OldStakeFlags::empty());
    let aligned = serialize_old_checked(&old_state);

    let mut backing = vec![0u8; LAYOUT_LEN + 1];
    backing[1..1 + LAYOUT_LEN].copy_from_slice(&aligned);

    // Overwrite tail in the unaligned region.
    backing[1 + FLAGS_OFF] = 238;
    backing[1 + PADDING_OFF..1 + LAYOUT_LEN].copy_from_slice(&[250, 206, 176]);
    let tail_before = backing[1 + FLAGS_OFF..1 + LAYOUT_LEN].to_vec();

    // Mutate using an unaligned &mut [u8]
    {
        let unaligned = &mut backing[1..1 + LAYOUT_LEN];
        let view = StakeStateV2ViewMut::from_bytes_mut(unaligned).unwrap();
        let StakeStateV2ViewMut::Stake { meta, stake } = view else {
            panic!("expected Stake");
        };

        meta.rent_exempt_reserve.set(999);
        stake.credits_observed.set(888);
        // ensure reserved still intact
        assert_eq!(stake.delegation._reserved, reserved_bytes);
    }

    // Tail preserved
    assert_eq!(&backing[1 + FLAGS_OFF..1 + LAYOUT_LEN], &tail_before[..]);
}
