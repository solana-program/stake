#![allow(deprecated)]
#![allow(clippy::arithmetic_side_effects)]

mod common;

use {
    common::*,
    p_stake_interface::{
        error::StakeStateError,
        state::{
            Authorized, Delegation, Lockup, Meta, PodAddress, PodI64, PodU64, Stake, StakeStateV2,
            StakeStateV2Tag, StakeStateV2View,
        },
    },
};

#[test]
fn given_short_buffer_when_writer_then_unexpected_eof() {
    let mut data = vec![0u8; 199];
    let Err(err) = StakeStateV2::from_bytes_mut(&mut data) else {
        panic!("expected error");
    };
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_invalid_tag_when_writer_then_invalid_tag() {
    let mut data = [0u8; 200];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());
    let Err(err) = StakeStateV2::from_bytes_mut(&mut data) else {
        panic!("expected error");
    };
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn given_trailing_bytes_when_writer_then_ok() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.push(0);
    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let view = writer.view().unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn given_unaligned_slice_when_writer_then_ok() {
    let mut data = vec![0u8; 201];
    let mut slice = &mut data[1..5];
    wincode::serialize_into(&mut slice, &StakeStateV2Tag::Uninitialized).unwrap();
    let unaligned = &mut data[1..201];

    let writer = StakeStateV2::from_bytes_mut(unaligned).unwrap();
    let view = writer.view().unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn given_uninitialized_bytes_when_into_initialized_then_zeroes_stake_and_tail() {
    let mut data = [0xAAu8; 200];
    let mut slice = &mut data[..4];
    wincode::serialize_into(&mut slice, &StakeStateV2Tag::Uninitialized).unwrap();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(42),
        authorized: Authorized {
            staker: PodAddress::from_bytes([1u8; 32]),
            withdrawer: PodAddress::from_bytes([2u8; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(-3),
            epoch: PodU64::from_primitive(9),
            custodian: PodAddress::from_bytes([3u8; 32]),
        },
    };

    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let _writer = writer.into_initialized(meta).unwrap();

    assert_tag(&data, StakeStateV2Tag::Initialized);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(view_meta) = view else {
        panic!("expected Initialized");
    };
    assert_eq!(view_meta.rent_exempt_reserve.get(), 42);
    assert_eq!(to_pubkey(&view_meta.authorized.staker), pk_u8(1));
    assert_eq!(to_pubkey(&view_meta.authorized.withdrawer), pk_u8(2));
    assert_eq!(view_meta.lockup.unix_timestamp.get(), -3);
    assert_eq!(view_meta.lockup.epoch.get(), 9);
    assert_eq!(to_pubkey(&view_meta.lockup.custodian), pk_u8(3));

    let stake_offset = 4 + core::mem::size_of::<Meta>();
    let stake_len = core::mem::size_of::<Stake>();
    assert!(data[stake_offset..stake_offset + stake_len]
        .iter()
        .all(|byte| *byte == 0));
    assert_eq!(data[196], 0);
    assert_eq!(&data[197..200], &[0u8; 3]);
}

#[test]
fn given_initialized_bytes_when_into_stake_then_zeroes_tail() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Initialized);
    data[196..200].copy_from_slice(&[0xAA; 4]);

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(7),
        authorized: Authorized {
            staker: PodAddress::from_bytes([4u8; 32]),
            withdrawer: PodAddress::from_bytes([5u8; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(6),
            epoch: PodU64::from_primitive(8),
            custodian: PodAddress::from_bytes([6u8; 32]),
        },
    };

    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: PodAddress::from_bytes([7u8; 32]),
            stake: PodU64::from_primitive(123),
            activation_epoch: PodU64::from_primitive(2),
            deactivation_epoch: PodU64::from_primitive(3),
            _reserved: [0xBB; 8],
        },
        credits_observed: PodU64::from_primitive(44),
    };

    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let _writer = writer.into_stake(meta, stake).unwrap();

    assert_tag(&data, StakeStateV2Tag::Stake);
    assert_eq!(data[196], 0);
    assert_eq!(&data[197..200], &[0u8; 3]);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { meta, stake, .. } = view else {
        panic!("expected Stake");
    };
    assert_eq!(meta.rent_exempt_reserve.get(), 7);
    assert_eq!(to_pubkey(&meta.authorized.staker), pk_u8(4));
    assert_eq!(to_pubkey(&meta.authorized.withdrawer), pk_u8(5));
    assert_eq!(meta.lockup.unix_timestamp.get(), 6);
    assert_eq!(meta.lockup.epoch.get(), 8);
    assert_eq!(to_pubkey(&meta.lockup.custodian), pk_u8(6));

    assert_eq!(to_pubkey(&stake.delegation.voter_pubkey), pk_u8(7));
    assert_eq!(stake.delegation.stake.get(), 123);
    assert_eq!(stake.delegation.activation_epoch.get(), 2);
    assert_eq!(stake.delegation.deactivation_epoch.get(), 3);
    assert_eq!(stake.delegation._reserved, [0xBB; 8]);
    assert_eq!(stake.credits_observed.get(), 44);
}

#[test]
fn given_stake_bytes_when_into_stake_then_preserves_tail() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Stake);
    data[196] = 0x5A;
    data[197..200].copy_from_slice(&[0xDE, 0xAD, 0xBE]);

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(1),
        authorized: Authorized {
            staker: PodAddress::from_bytes([9u8; 32]),
            withdrawer: PodAddress::from_bytes([8u8; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(7),
            epoch: PodU64::from_primitive(11),
            custodian: PodAddress::from_bytes([7u8; 32]),
        },
    };

    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: PodAddress::from_bytes([6u8; 32]),
            stake: PodU64::from_primitive(55),
            activation_epoch: PodU64::from_primitive(1),
            deactivation_epoch: PodU64::from_primitive(9),
            _reserved: [0xCC; 8],
        },
        credits_observed: PodU64::from_primitive(99),
    };

    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let _writer = writer.into_stake(meta, stake).unwrap();

    assert_tag(&data, StakeStateV2Tag::Stake);
    assert_eq!(data[196], 0x5A);
    assert_eq!(&data[197..200], &[0xDE, 0xAD, 0xBE]);
}

#[test]
fn given_rewards_pool_bytes_when_into_initialized_then_invalid_transition() {
    let mut data = empty_state_bytes(StakeStateV2Tag::RewardsPool);

    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let Err(err) = writer.into_initialized(Meta::default()) else {
        panic!("expected error");
    };
    assert!(matches!(
        err,
        StakeStateError::InvalidTransition {
            from: StakeStateV2Tag::RewardsPool,
            to: StakeStateV2Tag::Initialized,
        }
    ));
}

#[test]
fn given_uninitialized_bytes_when_into_stake_then_invalid_transition() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized);

    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let Err(err) = writer.into_stake(Meta::default(), Stake::default()) else {
        panic!("expected error");
    };
    assert!(matches!(
        err,
        StakeStateError::InvalidTransition {
            from: StakeStateV2Tag::Uninitialized,
            to: StakeStateV2Tag::Stake,
        }
    ));
}
