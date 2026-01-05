#![allow(deprecated)]
#![allow(clippy::arithmetic_side_effects)]

mod common;

use {
    bincode::Options,
    common::*,
    p_stake_interface::{error::StakeStateError, state::StakeStateV2Tag},
};

#[test]
fn given_tag_len_when_checked_then_is_4() {
    assert_eq!(StakeStateV2Tag::TAG_LEN, 4);
}

#[test]
fn given_tag_when_wincode_encoded_then_matches_u32_le() {
    fn check(tag: StakeStateV2Tag, expected: u32) {
        let mut buf = [0u8; 4];
        wincode::serialize_into(&mut buf.as_mut_slice(), &tag).unwrap();
        assert_eq!(buf, expected.to_le_bytes());
    }

    check(StakeStateV2Tag::Uninitialized, 0);
    check(StakeStateV2Tag::Initialized, 1);
    check(StakeStateV2Tag::Stake, 2);
    check(StakeStateV2Tag::RewardsPool, 3);
}

#[test]
fn given_u32_le_bytes_when_decoded_then_tag_matches() {
    for (expected, tag) in [
        (0u32, StakeStateV2Tag::Uninitialized),
        (1u32, StakeStateV2Tag::Initialized),
        (2u32, StakeStateV2Tag::Stake),
        (3u32, StakeStateV2Tag::RewardsPool),
    ] {
        let bytes = expected.to_le_bytes();
        let decoded = StakeStateV2Tag::from_bytes(&bytes[..]).unwrap();
        assert_eq!(decoded, tag);
    }
}

#[test]
fn given_tag_when_roundtrip_then_matches() {
    for tag in [
        StakeStateV2Tag::Uninitialized,
        StakeStateV2Tag::Initialized,
        StakeStateV2Tag::Stake,
        StakeStateV2Tag::RewardsPool,
    ] {
        let mut buf = [0u8; 4];
        wincode::serialize_into(&mut buf.as_mut_slice(), &tag).unwrap();
        let decoded = StakeStateV2Tag::from_bytes(&buf[..]).unwrap();
        assert_eq!(decoded, tag);
    }
}

#[test]
fn given_invalid_tag_bytes_when_decoded_then_invalid_tag() {
    let bytes = 999u32.to_le_bytes();
    let err = StakeStateV2Tag::from_bytes(&bytes[..]).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn given_short_tag_bytes_when_decoded_then_unexpected_eof() {
    let bytes = [0u8; 3];
    let err = StakeStateV2Tag::from_bytes(&bytes[..]).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_tag_when_bincode_discriminant_then_matches_wincode() {
    // Match the bincode options you already use for StakeStateV2 compatibility.
    let variants = [
        StakeStateV2Tag::Uninitialized,
        StakeStateV2Tag::Initialized,
        StakeStateV2Tag::Stake,
        StakeStateV2Tag::RewardsPool,
    ];

    for v in variants {
        // bincode: serialize the discriminant as a u32 (fixint tag encoding).
        let b = bincode_opts()
            .serialize(&(v as u32))
            .expect("bincode serialize");
        assert!(
            b.len() >= 4,
            "bincode encoding too short for {:?}: len={}",
            v,
            b.len()
        );
        let b_tag = &b[..4];

        // wincode: serialize the enum; your schema uses tag_encoding = "u32".
        let mut w = [0u8; 4];
        wincode::serialize_into(&mut w.as_mut_slice(), &v).expect("wincode serialize");

        assert!(
            w.len() >= 4,
            "wincode encoding too short for {:?}: len={}",
            v,
            w.len()
        );
        let w_tag = &w[..4];

        assert_eq!(
            b_tag, w_tag,
            "discriminant mismatch for {:?}: bincode={:02x?}, wincode={:02x?}",
            v, b_tag, w_tag
        );
    }
}
