#![allow(deprecated)]
#![allow(clippy::arithmetic_side_effects)]

mod common;

use {
    common::*,
    p_stake_interface::{
        error::StakeStateError,
        state::{
            Meta, StakeStateV2, StakeStateV2Layout, StakeStateV2Tag, StakeStateV2View,
            StakeStateV2ViewMut,
        },
    },
    solana_stake_interface::state::{
        Authorized as OldAuthorized, Lockup as OldLockup, Meta as OldMeta,
        StakeStateV2 as OldStakeStateV2,
    },
};

#[test]
fn given_layout_type_when_size_checked_then_200_bytes() {
    assert_eq!(core::mem::size_of::<StakeStateV2Layout>(), 200);
    assert_200_bytes(&empty_state_bytes(StakeStateV2Tag::Uninitialized));
}

#[test]
fn given_empty_buffer_when_view_then_unexpected_eof() {
    let data: [u8; 0] = [];
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_tag_only_buffer_when_view_then_unexpected_eof() {
    let mut data = [0u8; 4];
    wincode::serialize_into(&mut data.as_mut_slice(), &StakeStateV2Tag::Uninitialized).unwrap();
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_short_buffer_when_view_then_unexpected_eof() {
    let data = vec![0u8; 199];
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_trailing_bytes_when_view_then_ok() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.push(0);
    let view = StakeStateV2::from_bytes(&data).unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn given_unaligned_slice_when_view_then_ok() {
    let mut data = vec![0u8; 201];
    let mut slice = &mut data[1..5];
    wincode::serialize_into(&mut slice, &StakeStateV2Tag::Uninitialized).unwrap();
    let unaligned = &data[1..201];

    let view = StakeStateV2::from_bytes(unaligned).unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn given_trailing_bytes_when_view_mut_then_ok() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.push(0);
    let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
    assert!(matches!(view, StakeStateV2ViewMut::Uninitialized));
}

#[test]
fn given_invalid_tag_when_view_then_invalid_tag() {
    let mut data = [0u8; 200];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());

    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn given_invalid_tag_when_view_mut_then_invalid_tag() {
    let mut data = [0u8; 200];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());

    let err = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn given_initialized_bytes_when_view_then_borrows_expected_offsets() {
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

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    // Prove it's borrowing into the original buffer (no memcpy of MetaBytes).
    let meta_ptr = meta as *const Meta as *const u8;
    let expected_ptr = unsafe { data.as_ptr().add(4) };
    assert_eq!(meta_ptr, expected_ptr);

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
fn given_uninitialized_or_rewards_pool_bytes_when_view_then_tag_matches() {
    for (tag, expected) in [
        (StakeStateV2Tag::Uninitialized, "Uninitialized"),
        (StakeStateV2Tag::RewardsPool, "RewardsPool"),
    ] {
        let data = empty_state_bytes(tag);

        let view = StakeStateV2::from_bytes(&data).unwrap();
        match (view, expected) {
            (StakeStateV2View::Uninitialized, "Uninitialized") => {}
            (StakeStateV2View::RewardsPool, "RewardsPool") => {}
            _ => panic!("unexpected variant for tag {tag:?}"),
        }
    }
}
