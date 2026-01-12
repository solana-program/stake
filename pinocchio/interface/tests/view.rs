#![allow(clippy::arithmetic_side_effects)]
#![allow(deprecated)]

mod helpers;

use {
    bincode::Options,
    helpers::*,
    p_stake_interface::{
        error::StakeStateError,
        state::{StakeStateV2, StakeStateV2Tag, StakeStateV2View},
    },
    proptest::prelude::*,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as LegacyStakeFlags,
        state::{
            Authorized as LegacyAuthorized, Delegation as LegacyDelegation, Lockup as LegacyLockup,
            Meta as LegacyMeta, Stake as LegacyStake, StakeStateV2 as LegacyStakeStateV2,
        },
    },
    test_case::test_case,
};

// Verifies that the deserialized view is a true zero-copy borrow into the original byte slice.
fn assert_borrows_at<T>(borrow: &T, bytes: &[u8], offset: usize) {
    let ptr = borrow as *const T;
    let expected = unsafe { bytes.as_ptr().add(offset) };
    assert_eq!(ptr as *const u8, expected);
}

fn overwrite_tail(bytes: &mut [u8], stake_flags: u8, padding: [u8; 3]) {
    bytes[FLAGS_OFF] = stake_flags;
    bytes[PADDING_OFF..LAYOUT_LEN].copy_from_slice(&padding);
}

#[test]
fn view_short_buffer_returns_unexpected_eof() {
    let data = vec![0u8; LAYOUT_LEN - 1];
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn view_buffer_with_trailing_bytes_is_ok() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.extend_from_slice(&[171; 64]);
    let view = StakeStateV2::from_bytes(&data).unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
}

#[test]
fn view_invalid_tag_returns_error() {
    let mut data = [0u8; LAYOUT_LEN];
    data[..4].copy_from_slice(&999u32.to_le_bytes());
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test_case(StakeStateV2Tag::Uninitialized)]
#[test_case(StakeStateV2Tag::Initialized)]
#[test_case(StakeStateV2Tag::Stake)]
#[test_case(StakeStateV2Tag::RewardsPool)]
fn view_variants_match_tag_for_empty_layout_bytes(tag: StakeStateV2Tag) {
    let data = empty_state_bytes(tag);
    let bytes = &data;

    let view = StakeStateV2::from_bytes(bytes).unwrap();
    match (tag, view) {
        (StakeStateV2Tag::Uninitialized, StakeStateV2View::Uninitialized) => {}
        (StakeStateV2Tag::RewardsPool, StakeStateV2View::RewardsPool) => {}

        (StakeStateV2Tag::Initialized, StakeStateV2View::Initialized(meta)) => {
            assert_borrows_at(meta, bytes, META_OFF);
        }

        (StakeStateV2Tag::Stake, StakeStateV2View::Stake { meta, stake }) => {
            assert_borrows_at(meta, bytes, META_OFF);
            assert_borrows_at(stake, bytes, STAKE_OFF);
        }

        _ => panic!("unexpected variant for tag {tag:?}"),
    }
}

#[test]
fn view_initialized_legacy_bytes_borrows_correctly() {
    let legacy_meta = LegacyMeta {
        rent_exempt_reserve: 1234605616436508552,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([1u8; 32]),
            withdrawer: Pubkey::new_from_array([2u8; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: -123,
            epoch: 456,
            custodian: Pubkey::new_from_array([3u8; 32]),
        },
    };
    let legacy_state = LegacyStakeStateV2::Initialized(legacy_meta);
    let data = serialize_legacy(&legacy_state);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    assert_borrows_at(meta, &data, META_OFF);
    assert_meta_compat(meta, &legacy_meta);

    // Legacy encoding defaults for these bytes in Initialized.
    assert_eq!(data[FLAGS_OFF], 0);
    assert_eq!(&data[PADDING_OFF..LAYOUT_LEN], &[0u8; 3]);
}

#[test]
fn view_unaligned_initialized_legacy_bytes_borrows_correctly() {
    let legacy_meta = LegacyMeta {
        rent_exempt_reserve: 42,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([17; 32]),
            withdrawer: Pubkey::new_from_array([34; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: i64::MIN + 7,
            epoch: u64::MAX - 9,
            custodian: Pubkey::new_from_array([51; 32]),
        },
    };
    let legacy_state = LegacyStakeStateV2::Initialized(legacy_meta);
    let aligned = serialize_legacy(&legacy_state);

    let mut buffer = vec![0u8; LAYOUT_LEN + 1];
    buffer[1..1 + LAYOUT_LEN].copy_from_slice(&aligned);
    let unaligned = &buffer[1..1 + LAYOUT_LEN];

    let view = StakeStateV2::from_bytes(unaligned).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    assert_borrows_at(meta, unaligned, META_OFF);
    assert_meta_compat(meta, &legacy_meta);
}

#[test]
fn view_initialized_legacy_bytes_ignores_tail_bytes() {
    let legacy_meta = LegacyMeta {
        rent_exempt_reserve: 1,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([9u8; 32]),
            withdrawer: Pubkey::new_from_array([8u8; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: -7,
            epoch: 9,
            custodian: Pubkey::new_from_array([7u8; 32]),
        },
    };
    let legacy_state = LegacyStakeStateV2::Initialized(legacy_meta);
    let mut data = serialize_legacy(&legacy_state);

    overwrite_tail(&mut data, 0xDE, [0xAD, 0xBE, 0xEF]);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Initialized(meta) = view else {
        panic!("expected Initialized");
    };

    assert_meta_compat(meta, &legacy_meta);
    assert_eq!(data[FLAGS_OFF], 0xDE);
    assert_eq!(&data[PADDING_OFF..LAYOUT_LEN], &[0xAD, 0xBE, 0xEF]);
}

#[test]
fn view_stake_legacy_bytes_borrows_correctly() {
    let legacy_meta = LegacyMeta {
        rent_exempt_reserve: 1,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([68; 32]),
            withdrawer: Pubkey::new_from_array([85; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: -1,
            epoch: 1,
            custodian: Pubkey::new_from_array([102; 32]),
        },
    };

    let reserved_bytes = [170u8; 8];
    let legacy_delegation = LegacyDelegation {
        voter_pubkey: Pubkey::new_from_array([119; 32]),
        stake: u64::MAX,
        activation_epoch: 0,
        deactivation_epoch: u64::MAX,
        warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
    };

    let legacy_stake = LegacyStake {
        delegation: legacy_delegation,
        credits_observed: u64::MAX - 1,
    };

    let mut legacy_flags = LegacyStakeFlags::empty();
    legacy_flags.set(LegacyStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);

    let expected_flags_byte = bincode_opts().serialize(&legacy_flags).unwrap()[0];

    let legacy_state = LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, legacy_flags);
    let data = serialize_legacy(&legacy_state);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };

    assert_borrows_at(meta, &data, META_OFF);
    assert_borrows_at(stake, &data, STAKE_OFF);

    assert_meta_compat(meta, &legacy_meta);
    assert_stake_compat(stake, &legacy_stake);

    assert_eq!(data[FLAGS_OFF], expected_flags_byte);
}

#[test]
fn view_unaligned_stake_legacy_bytes_borrows_correctly() {
    let legacy_meta = LegacyMeta {
        rent_exempt_reserve: 9,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([1; 32]),
            withdrawer: Pubkey::new_from_array([2; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: 3,
            epoch: 4,
            custodian: Pubkey::new_from_array([5; 32]),
        },
    };

    let reserved_bytes = [94u8; 8];
    let legacy_delegation = LegacyDelegation {
        voter_pubkey: Pubkey::new_from_array([6; 32]),
        stake: 7,
        activation_epoch: 8,
        deactivation_epoch: 9,
        warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
    };
    let legacy_stake = LegacyStake {
        delegation: legacy_delegation,
        credits_observed: 10,
    };

    let legacy_state =
        LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, LegacyStakeFlags::empty());
    let aligned = serialize_legacy(&legacy_state);

    let mut buffer = vec![0u8; LAYOUT_LEN + 1];
    buffer[1..1 + LAYOUT_LEN].copy_from_slice(&aligned);
    let unaligned = &buffer[1..1 + LAYOUT_LEN];

    let view = StakeStateV2::from_bytes(unaligned).unwrap();
    let StakeStateV2View::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };

    assert_borrows_at(meta, unaligned, META_OFF);
    assert_borrows_at(stake, unaligned, STAKE_OFF);

    assert_meta_compat(meta, &legacy_meta);
    assert_stake_compat(stake, &legacy_stake);

    assert_eq!(aligned[FLAGS_OFF], 0);
}

#[test]
fn view_stake_legacy_bytes_ignores_tail_bytes() {
    let legacy_meta = LegacyMeta {
        rent_exempt_reserve: 111,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([17; 32]),
            withdrawer: Pubkey::new_from_array([34; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: -7,
            epoch: 9,
            custodian: Pubkey::new_from_array([51; 32]),
        },
    };

    let reserved_bytes = [19u8; 8];
    let legacy_delegation = LegacyDelegation {
        voter_pubkey: Pubkey::new_from_array([6; 32]),
        stake: 7,
        activation_epoch: 8,
        deactivation_epoch: 9,
        warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
    };
    let legacy_stake = LegacyStake {
        delegation: legacy_delegation,
        credits_observed: 10,
    };

    let legacy_state =
        LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, LegacyStakeFlags::empty());
    let mut data = serialize_legacy(&legacy_state);

    overwrite_tail(&mut data, 0xEE, [0xFA, 0xCE, 0xB0]);

    let view = StakeStateV2::from_bytes(&data).unwrap();
    let StakeStateV2View::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };

    assert_meta_compat(meta, &legacy_meta);
    assert_stake_compat(stake, &legacy_stake);

    assert_eq!(data[FLAGS_OFF], 0xEE);
    assert_eq!(&data[PADDING_OFF..LAYOUT_LEN], &[0xFA, 0xCE, 0xB0]);
}

// ----------------------------- property tests --------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    #[test]
    fn prop_short_buffer_returns_unexpected_eof(data in proptest::collection::vec(any::<u8>(), 0..LAYOUT_LEN)) {
        let err = StakeStateV2::from_bytes(&data).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::UnexpectedEof));
    }

    #[test]
    fn prop_invalid_tag_when_view_then_invalid_tag(
        mut bytes in any::<[u8; 200]>(),
        invalid in 4u32..=u32::MAX,
    ) {
        bytes[..4].copy_from_slice(&invalid.to_le_bytes());
        let err = StakeStateV2::from_bytes(&bytes).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::InvalidTag(t) if t == invalid));
    }

    #[test]
    fn prop_any_200_bytes_with_valid_tag_when_view_then_variant_matches(
        mut bytes in any::<[u8; 200]>(),
        tag in arb_valid_tag(),
    ) {
        write_tag(&mut bytes, tag);

        let view = StakeStateV2::from_bytes(&bytes).unwrap();
        match (tag, view) {
            (StakeStateV2Tag::Uninitialized, StakeStateV2View::Uninitialized) => {}
            (StakeStateV2Tag::Initialized, StakeStateV2View::Initialized(_)) => {}
            (StakeStateV2Tag::Stake, StakeStateV2View::Stake { .. }) => {}
            (StakeStateV2Tag::RewardsPool, StakeStateV2View::RewardsPool) => {}
            _ => prop_assert!(false, "tag/view mismatch"),
        }
    }

    #[test]
    fn prop_any_200_bytes_with_valid_tag_when_view_then_variant_matches_on_unaligned_slice(
        mut buffer in any::<[u8; 201]>(),
        tag in arb_valid_tag(),
    ) {
        // Make an unaligned 200-byte window
        let unaligned = &mut buffer[1..1 + LAYOUT_LEN];
        write_tag(unaligned, tag);

        let view = StakeStateV2::from_bytes(unaligned).unwrap();
        match (tag, view) {
            (StakeStateV2Tag::Uninitialized, StakeStateV2View::Uninitialized) => {}
            (StakeStateV2Tag::Initialized, StakeStateV2View::Initialized(meta)) => {
                assert_borrows_at(meta, unaligned, META_OFF);
            }
            (StakeStateV2Tag::Stake, StakeStateV2View::Stake { meta, stake }) => {
                assert_borrows_at(meta, unaligned, META_OFF);
                assert_borrows_at(stake, unaligned, STAKE_OFF);
            }
            (StakeStateV2Tag::RewardsPool, StakeStateV2View::RewardsPool) => {}
            _ => prop_assert!(false, "tag/view mismatch (unaligned)"),
        }
    }

    #[test]
    fn prop_random_legacy_state_when_view_then_matches_expected(legacy in arb_legacy_state()) {
        let data = serialize_legacy(&legacy);
        prop_assert_eq!(data.len(), 200);

        let view = StakeStateV2::from_bytes(&data).unwrap();
        match (legacy, view) {
            (LegacyStakeStateV2::Uninitialized, StakeStateV2View::Uninitialized) => {}
            (LegacyStakeStateV2::RewardsPool, StakeStateV2View::RewardsPool) => {}
            (LegacyStakeStateV2::Initialized(legacy_meta), StakeStateV2View::Initialized(meta)) => {
                assert_meta_compat(meta, &legacy_meta);
            }
            (LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, flags), StakeStateV2View::Stake { meta, stake }) => {
                assert_meta_compat(meta, &legacy_meta);
                assert_stake_compat(stake, &legacy_stake);
                prop_assert_eq!(data[FLAGS_OFF], stake_flags_byte(&flags));
            }
            (o, v) => prop_assert!(false, "variant mismatch legacy={o:?} new={v:?}"),
        }
    }
}
