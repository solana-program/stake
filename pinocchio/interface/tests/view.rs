#![allow(clippy::arithmetic_side_effects)]
#![allow(deprecated)]

mod helpers;

use {
    bincode::Options,
    core::mem::{align_of, size_of},
    helpers::*,
    p_stake_interface::{
        error::StakeStateError,
        state::{Meta, Stake, StakeStateV2, StakeStateV2Tag},
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

#[test]
fn len_constant_is_200() {
    assert_eq!(StakeStateV2::LEN, 200);
    assert_eq!(StakeStateV2::LEN, STATE_LEN);
}

#[test]
fn layout_invariants() {
    // Alignment must be 1 for safe zero-copy from unaligned slices
    assert_eq!(align_of::<StakeStateV2>(), 1);
    assert_eq!(align_of::<Meta>(), 1);
    assert_eq!(align_of::<Stake>(), 1);

    // Struct sizes must match documented layout
    assert_eq!(size_of::<StakeStateV2>(), 200);
    assert_eq!(size_of::<Meta>(), 120);
    assert_eq!(size_of::<Stake>(), 72);

    // Offsets must match documented layout table
    assert_eq!(TAG_LEN, 4);
    assert_eq!(META_OFFSET, 4);
    assert_eq!(STAKE_OFFSET, 124); // 4 + 120
    assert_eq!(PADDING_OFFSET, 196); // 4 + 120 + 72
}

#[test]
fn short_buffer_returns_decode() {
    let data = vec![0u8; STATE_LEN - 1];
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::Decode));
}

#[test]
fn buffer_with_trailing_bytes_is_ok() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.extend_from_slice(&[171; 64]);
    let layout = StakeStateV2::from_bytes(&data).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Uninitialized);
    assert!(layout.meta().is_err());
}

#[test]
fn invalid_tag_returns_error() {
    let mut data = [0u8; STATE_LEN];
    data[..4].copy_from_slice(&999u32.to_le_bytes());
    let err = StakeStateV2::from_bytes(&data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test_case(StakeStateV2Tag::Uninitialized)]
#[test_case(StakeStateV2Tag::Initialized)]
#[test_case(StakeStateV2Tag::Stake)]
#[test_case(StakeStateV2Tag::RewardsPool)]
fn variants_match_tag_for_empty_state_bytes(tag: StakeStateV2Tag) {
    let data = empty_state_bytes(tag);
    let bytes = &data;

    let layout = StakeStateV2::from_bytes(bytes).unwrap();
    assert_eq!(layout.tag(), tag);

    match tag {
        StakeStateV2Tag::Uninitialized | StakeStateV2Tag::RewardsPool => {
            assert!(layout.meta().is_err());
            assert!(layout.stake().is_err());
        }
        StakeStateV2Tag::Initialized => {
            let meta = layout.meta().unwrap();
            assert_borrows_at(meta, bytes, META_OFFSET);
            assert!(layout.stake().is_err());
        }
        StakeStateV2Tag::Stake => {
            let meta = layout.meta().unwrap();
            let stake = layout.stake().unwrap();
            assert_borrows_at(meta, bytes, META_OFFSET);
            assert_borrows_at(stake, bytes, STAKE_OFFSET);
        }
    }
}

#[test]
fn initialized_legacy_bytes_borrows_correctly() {
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

    let layout = StakeStateV2::from_bytes(&data).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);

    let meta = layout.meta().unwrap();
    assert_borrows_at(meta, &data, META_OFFSET);
    assert_meta_compat(meta, &legacy_meta);

    // Legacy encoding defaults for these bytes in Initialized.
    assert_eq!(&data[PADDING_OFFSET..STATE_LEN], &[0u8; 4]);
}

#[test]
fn unaligned_initialized_legacy_bytes_borrows_correctly() {
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

    let mut buffer = vec![0u8; STATE_LEN + 1];
    buffer[1..1 + STATE_LEN].copy_from_slice(&aligned);
    let unaligned = &buffer[1..1 + STATE_LEN];

    let layout = StakeStateV2::from_bytes(unaligned).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);

    let meta = layout.meta().unwrap();
    assert_borrows_at(meta, unaligned, META_OFFSET);
    assert_meta_compat(meta, &legacy_meta);
}

#[test]
fn initialized_legacy_bytes_ignores_tail_bytes() {
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

    overwrite_tail(&mut data, [0xDE, 0xAD, 0xBE, 0xEF]);

    let layout = StakeStateV2::from_bytes(&data).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);

    let meta = layout.meta().unwrap();
    assert_meta_compat(meta, &legacy_meta);
    assert_eq!(&data[PADDING_OFFSET..STATE_LEN], &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn stake_legacy_bytes_borrows_correctly() {
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

    let layout = StakeStateV2::from_bytes(&data).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Stake);

    let meta = layout.meta().unwrap();
    let stake = layout.stake().unwrap();

    assert_borrows_at(meta, &data, META_OFFSET);
    assert_borrows_at(stake, &data, STAKE_OFFSET);

    assert_meta_compat(meta, &legacy_meta);
    assert_stake_compat(stake, &legacy_stake);

    assert_eq!(data[PADDING_OFFSET], expected_flags_byte);
}

#[test]
fn unaligned_stake_legacy_bytes_borrows_correctly() {
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

    let mut buffer = vec![0u8; STATE_LEN + 1];
    buffer[1..1 + STATE_LEN].copy_from_slice(&aligned);
    let unaligned = &buffer[1..1 + STATE_LEN];

    let layout = StakeStateV2::from_bytes(unaligned).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Stake);

    let meta = layout.meta().unwrap();
    let stake = layout.stake().unwrap();

    assert_borrows_at(meta, unaligned, META_OFFSET);
    assert_borrows_at(stake, unaligned, STAKE_OFFSET);

    assert_meta_compat(meta, &legacy_meta);
    assert_stake_compat(stake, &legacy_stake);

    assert_eq!(aligned[PADDING_OFFSET], 0);
}

#[test]
fn stake_legacy_bytes_ignores_tail_bytes() {
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

    overwrite_tail(&mut data, [0xEE, 0xFA, 0xCE, 0xB0]);

    let layout = StakeStateV2::from_bytes(&data).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Stake);

    let meta = layout.meta().unwrap();
    let stake = layout.stake().unwrap();

    assert_meta_compat(meta, &legacy_meta);
    assert_stake_compat(stake, &legacy_stake);

    assert_eq!(&data[PADDING_OFFSET..STATE_LEN], &[0xEE, 0xFA, 0xCE, 0xB0]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_short_buffer_returns_decode(data in proptest::collection::vec(any::<u8>(), 0..STATE_LEN)) {
        let err = StakeStateV2::from_bytes(&data).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::Decode));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_invalid_tag_returns_error(
        mut bytes in any::<[u8; 200]>(),
        invalid in 4u32..=u32::MAX,
    ) {
        bytes[..4].copy_from_slice(&invalid.to_le_bytes());
        let err = StakeStateV2::from_bytes(&bytes).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::InvalidTag(t) if t == invalid));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_valid_tag_parses_correctly(
        mut bytes in any::<[u8; 200]>(),
        tag in arb_valid_tag(),
    ) {
        write_tag(&mut bytes, tag);

        let layout = StakeStateV2::from_bytes(&bytes).unwrap();
        prop_assert_eq!(layout.tag(), tag);

        match tag {
            StakeStateV2Tag::Uninitialized | StakeStateV2Tag::RewardsPool => {
                prop_assert!(layout.meta().is_err());
                prop_assert!(layout.stake().is_err());
            }
            StakeStateV2Tag::Initialized => {
                prop_assert!(layout.meta().is_ok());
                prop_assert!(layout.stake().is_err());
            }
            StakeStateV2Tag::Stake => {
                prop_assert!(layout.meta().is_ok());
                prop_assert!(layout.stake().is_ok());
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_valid_tag_parses_correctly_unaligned(
        mut buffer in any::<[u8; 201]>(),
        tag in arb_valid_tag(),
    ) {
        // Make an unaligned 200-byte window
        let unaligned = &mut buffer[1..1 + STATE_LEN];
        write_tag(unaligned, tag);

        let layout = StakeStateV2::from_bytes(unaligned).unwrap();
        prop_assert_eq!(layout.tag(), tag);

        match tag {
            StakeStateV2Tag::Uninitialized | StakeStateV2Tag::RewardsPool => {
                prop_assert!(layout.meta().is_err());
                prop_assert!(layout.stake().is_err());
            }
            StakeStateV2Tag::Initialized => {
                let meta = layout.meta().unwrap();
                assert_borrows_at(meta, unaligned, META_OFFSET);
                prop_assert!(layout.stake().is_err());
            }
            StakeStateV2Tag::Stake => {
                let meta = layout.meta().unwrap();
                let stake = layout.stake().unwrap();
                assert_borrows_at(meta, unaligned, META_OFFSET);
                assert_borrows_at(stake, unaligned, STAKE_OFFSET);
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_legacy_state_roundtrips(legacy in arb_legacy_state()) {
        let data = serialize_legacy(&legacy);
        prop_assert_eq!(data.len(), 200);

        let layout = StakeStateV2::from_bytes(&data).unwrap();

        match legacy {
            LegacyStakeStateV2::Uninitialized => {
                prop_assert_eq!(layout.tag(), StakeStateV2Tag::Uninitialized);
                prop_assert!(layout.meta().is_err());
                prop_assert!(layout.stake().is_err());
            }
            LegacyStakeStateV2::RewardsPool => {
                prop_assert_eq!(layout.tag(), StakeStateV2Tag::RewardsPool);
                prop_assert!(layout.meta().is_err());
                prop_assert!(layout.stake().is_err());
            }
            LegacyStakeStateV2::Initialized(legacy_meta) => {
                prop_assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);
                let meta = layout.meta().unwrap();
                assert_meta_compat(meta, &legacy_meta);
                prop_assert!(layout.stake().is_err());
            }
            LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, flags) => {
                prop_assert_eq!(layout.tag(), StakeStateV2Tag::Stake);
                let meta = layout.meta().unwrap();
                let stake = layout.stake().unwrap();
                assert_meta_compat(meta, &legacy_meta);
                assert_stake_compat(stake, &legacy_stake);
                prop_assert_eq!(data[PADDING_OFFSET], stake_flags_byte(&flags));
            }
        }
    }
}
