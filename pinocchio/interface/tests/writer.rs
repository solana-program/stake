#![allow(clippy::arithmetic_side_effects)]
#![allow(deprecated)]

mod helpers;

use {
    helpers::*,
    p_stake_interface::{
        error::StakeStateError,
        state::{
            Authorized, Delegation, Lockup, Meta, PodAddress, PodI64, PodU64, Stake, StakeStateV2,
            StakeStateV2Layout, StakeStateV2Tag, StakeStateV2View, StakeStateV2ViewMut,
        },
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
    wincode::ZeroCopy,
};

fn assert_tail_zeroed(layout_bytes: &[u8]) {
    assert_eq!(layout_bytes[FLAGS_OFF], 0);
    assert_eq!(&layout_bytes[PADDING_OFF..LAYOUT_LEN], &[0u8; 3]);
}

fn assert_tail(layout_bytes: &[u8], flags: u8, padding: [u8; 3]) {
    assert_eq!(layout_bytes[FLAGS_OFF], flags);
    assert_eq!(&layout_bytes[PADDING_OFF..LAYOUT_LEN], &padding);
}

#[test]
fn short_buffer_eof() {
    let mut data = vec![0u8; LAYOUT_LEN - 1];
    let err = StakeStateV2::from_bytes_mut(&mut data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn invalid_tag_err() {
    let mut data = [0u8; LAYOUT_LEN];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());
    let err = StakeStateV2::from_bytes_mut(&mut data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn trailing_bytes_untouched() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.extend_from_slice(&[171; 64]);

    let expected = data.clone();

    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let view = writer.view().unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
    assert_eq!(data, expected);
}

#[test]
fn unaligned_slice_noop() {
    let mut buffer = vec![238u8; LAYOUT_LEN + 1];
    write_tag(&mut buffer[1..1 + TAG_LEN], StakeStateV2Tag::Uninitialized);

    let expected = buffer.clone();
    let unaligned = &mut buffer[1..1 + LAYOUT_LEN];

    let writer = StakeStateV2::from_bytes_mut(unaligned).unwrap();
    let view = writer.view().unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));
    assert_eq!(buffer, expected);
}

#[test]
fn legacy_bytes_unchanged() {
    let legacy_initialized = LegacyStakeStateV2::Initialized(LegacyMeta {
        rent_exempt_reserve: u64::MAX,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([17; 32]),
            withdrawer: Pubkey::new_from_array([34; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: i64::MIN + 1,
            epoch: u64::MAX,
            custodian: Pubkey::new_from_array([51; 32]),
        },
    });

    let legacy_flags = LegacyStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
    let legacy_stake = LegacyStakeStateV2::Stake(
        LegacyMeta {
            rent_exempt_reserve: 7,
            authorized: LegacyAuthorized {
                staker: Pubkey::new_from_array([68; 32]),
                withdrawer: Pubkey::new_from_array([85; 32]),
            },
            lockup: LegacyLockup {
                unix_timestamp: -9,
                epoch: 123,
                custodian: Pubkey::new_from_array([102; 32]),
            },
        },
        LegacyStake {
            delegation: LegacyDelegation {
                voter_pubkey: Pubkey::new_from_array([119; 32]),
                stake: 999,
                activation_epoch: 1,
                deactivation_epoch: 2,
                warmup_cooldown_rate: 0.5,
            },
            credits_observed: 88,
        },
        legacy_flags,
    );

    let variants = [
        LegacyStakeStateV2::Uninitialized,
        legacy_initialized,
        legacy_stake,
        LegacyStakeStateV2::RewardsPool,
    ];

    for legacy_state in variants {
        let mut data = serialize_legacy(&legacy_state);

        data[FLAGS_OFF] = 165;
        data[PADDING_OFF..LAYOUT_LEN].copy_from_slice(&[222, 173, 190]);
        data[STAKE_OFF..STAKE_OFF + 8].copy_from_slice(&[204; 8]);

        let expected = data.clone();
        StakeStateV2::from_bytes_mut(&mut data).unwrap();
        assert_eq!(data, expected);
    }
}

#[test_case(false, 0; "aligned_no_trailing")]
#[test_case(false, 64; "aligned_trailing")]
#[test_case(true, 0; "unaligned_no_trailing")]
#[test_case(true, 64; "unaligned_trailing")]
fn invalid_transitions_err(unaligned: bool, trailing_len: usize) {
    #[derive(Clone, Copy)]
    enum Op {
        IntoInitialized,
        IntoStake,
    }

    let tags = [
        StakeStateV2Tag::Uninitialized,
        StakeStateV2Tag::Initialized,
        StakeStateV2Tag::Stake,
        StakeStateV2Tag::RewardsPool,
    ];

    let start = if unaligned { 1 } else { 0 };

    for from in tags {
        for &op in &[Op::IntoInitialized, Op::IntoStake] {
            let mut base = empty_state_bytes(from);
            base[FLAGS_OFF..LAYOUT_LEN].copy_from_slice(&[250, 251, 252, 253]);

            let mut buffer = vec![238u8; start + LAYOUT_LEN + trailing_len];
            buffer[start..start + LAYOUT_LEN].copy_from_slice(&base);
            buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(123);

            let expected_layout = buffer[start..start + LAYOUT_LEN].to_vec();
            let expected_trailing =
                buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

            let result = {
                let slice = &mut buffer[start..start + LAYOUT_LEN + trailing_len];
                let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
                match op {
                    Op::IntoInitialized => writer.into_initialized(Meta::default()).map(|_| ()),
                    Op::IntoStake => writer
                        .into_stake(Meta::default(), Stake::default())
                        .map(|_| ()),
                }
            };

            let allowed = matches!(
                (from, op),
                (StakeStateV2Tag::Uninitialized, Op::IntoInitialized)
                    | (StakeStateV2Tag::Initialized, Op::IntoStake)
                    | (StakeStateV2Tag::Stake, Op::IntoStake)
            );

            if allowed {
                assert!(result.is_ok());
            } else {
                let expected_to = match op {
                    Op::IntoInitialized => StakeStateV2Tag::Initialized,
                    Op::IntoStake => StakeStateV2Tag::Stake,
                };
                let err = result.unwrap_err();
                assert!(matches!(
                    err,
                    StakeStateError::InvalidTransition { from: f, to: t }
                        if f == from && t == expected_to
                ));
                assert_eq!(
                    &buffer[start..start + LAYOUT_LEN],
                    expected_layout.as_slice()
                );
                assert_eq!(
                    &buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
                    expected_trailing.as_slice()
                );
            }
        }
    }
}

#[test_case(false, 0; "aligned_no_trailing")]
#[test_case(false, 64; "aligned_trailing")]
#[test_case(true, 0; "unaligned_no_trailing")]
#[test_case(true, 64; "unaligned_trailing")]
fn uninitialized_to_initialized(unaligned: bool, trailing_len: usize) {
    let mut base = [170u8; LAYOUT_LEN];
    write_tag(&mut base, StakeStateV2Tag::Uninitialized);

    let start = if unaligned { 1 } else { 0 };
    let mut buffer = vec![238u8; start + LAYOUT_LEN + trailing_len];
    buffer[start..start + LAYOUT_LEN].copy_from_slice(&base);
    buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(124);

    let expected_trailing = buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(42),
        authorized: Authorized {
            staker: PodAddress::from_bytes([1; 32]),
            withdrawer: PodAddress::from_bytes([2; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(-3),
            epoch: PodU64::from_primitive(9),
            custodian: PodAddress::from_bytes([3; 32]),
        },
    };

    let slice = &mut buffer[start..start + LAYOUT_LEN + trailing_len];
    let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
    writer.into_initialized(meta).unwrap();

    assert_eq!(
        &buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
        expected_trailing.as_slice()
    );

    let layout_bytes = &buffer[start..start + LAYOUT_LEN];

    assert_eq!(
        StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
        StakeStateV2Tag::Initialized
    );
    assert!(layout_bytes[STAKE_OFF..FLAGS_OFF].iter().all(|b| *b == 0));

    assert_tail_zeroed(layout_bytes);

    let view = StakeStateV2::from_bytes(layout_bytes).unwrap();
    let StakeStateV2View::Initialized(view_meta) = view else {
        panic!("expected Initialized");
    };

    let expected_legacy = LegacyMeta {
        rent_exempt_reserve: 42,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([1; 32]),
            withdrawer: Pubkey::new_from_array([2; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: -3,
            epoch: 9,
            custodian: Pubkey::new_from_array([3; 32]),
        },
    };
    assert_meta_compat(view_meta, &expected_legacy);

    let old = deserialize_legacy(layout_bytes);
    let LegacyStakeStateV2::Initialized(legacy_meta) = old else {
        panic!("expected legacy Initialized");
    };
    assert_eq!(legacy_meta, expected_legacy);
}

#[test_case(false, 0; "aligned_no_trailing")]
#[test_case(false, 64; "aligned_trailing")]
#[test_case(true, 0; "unaligned_no_trailing")]
#[test_case(true, 64; "unaligned_trailing")]
fn initialized_to_stake(unaligned: bool, trailing_len: usize) {
    let mut base = empty_state_bytes(StakeStateV2Tag::Initialized);
    base[FLAGS_OFF..LAYOUT_LEN].copy_from_slice(&[170, 187, 204, 221]);

    let start = if unaligned { 1 } else { 0 };
    let mut buffer = vec![238u8; start + LAYOUT_LEN + trailing_len];
    buffer[start..start + LAYOUT_LEN].copy_from_slice(&base);
    buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(125);

    let expected_trailing = buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(7),
        authorized: Authorized {
            staker: PodAddress::from_bytes([4; 32]),
            withdrawer: PodAddress::from_bytes([5; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(6),
            epoch: PodU64::from_primitive(8),
            custodian: PodAddress::from_bytes([6; 32]),
        },
    };
    let warmup_rate: f64 = 1.0;
    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: PodAddress::from_bytes([7; 32]),
            stake: PodU64::from_primitive(123),
            activation_epoch: PodU64::from_primitive(2),
            deactivation_epoch: PodU64::from_primitive(3),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(44),
    };

    let slice = &mut buffer[start..start + LAYOUT_LEN + trailing_len];
    let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
    writer.into_stake(meta, stake).unwrap();

    assert_eq!(
        &buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
        expected_trailing.as_slice()
    );

    let layout_bytes = &buffer[start..start + LAYOUT_LEN];
    assert_eq!(
        StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
        StakeStateV2Tag::Stake
    );

    assert_tail_zeroed(layout_bytes);

    let view = StakeStateV2::from_bytes(layout_bytes).unwrap();
    let StakeStateV2View::Stake { meta, stake, .. } = view else {
        panic!("expected Stake");
    };

    // Expected legacy values matching inputs above
    let expected_legacy = LegacyMeta {
        rent_exempt_reserve: 7,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([4; 32]),
            withdrawer: Pubkey::new_from_array([5; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: 6,
            epoch: 8,
            custodian: Pubkey::new_from_array([6; 32]),
        },
    };
    let expected_legacy_stake = LegacyStake {
        delegation: LegacyDelegation {
            voter_pubkey: Pubkey::new_from_array([7; 32]),
            stake: 123,
            activation_epoch: 2,
            deactivation_epoch: 3,
            warmup_cooldown_rate: warmup_rate,
        },
        credits_observed: 44,
    };

    assert_meta_compat(meta, &expected_legacy);
    assert_stake_compat(stake, &expected_legacy_stake);

    let old = deserialize_legacy(layout_bytes);
    let LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, legacy_flags) = old else {
        panic!("expected legacy Stake");
    };
    assert_eq!(legacy_meta, expected_legacy);
    assert_eq!(legacy_stake, expected_legacy_stake);
    assert_eq!(legacy_flags, LegacyStakeFlags::empty());
}

#[test_case(false, 0; "aligned_no_trailing")]
#[test_case(false, 64; "aligned_trailing")]
#[test_case(true, 0; "unaligned_no_trailing")]
#[test_case(true, 64; "unaligned_trailing")]
fn initialized_to_stake_view_mut_works(unaligned: bool, trailing_len: usize) {
    // Start from Initialized with a nonzero tail to ensure the transition zeroes it,
    // and then ensure subsequent view_mut does not modify it
    let mut base = empty_state_bytes(StakeStateV2Tag::Initialized);
    base[FLAGS_OFF..LAYOUT_LEN].copy_from_slice(&[170, 187, 204, 221]);

    let start = if unaligned { 1 } else { 0 };
    let mut buffer = vec![238u8; start + LAYOUT_LEN + trailing_len];
    buffer[start..start + LAYOUT_LEN].copy_from_slice(&base);
    buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(99);

    let trailing_before = buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(7),
        authorized: Authorized {
            staker: PodAddress::from_bytes([4; 32]),
            withdrawer: PodAddress::from_bytes([5; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(6),
            epoch: PodU64::from_primitive(8),
            custodian: PodAddress::from_bytes([6; 32]),
        },
    };
    let warmup_rate: f64 = 1.0;
    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: PodAddress::from_bytes([7; 32]),
            stake: PodU64::from_primitive(123),
            activation_epoch: PodU64::from_primitive(2),
            deactivation_epoch: PodU64::from_primitive(3),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(44),
    };

    let slice = &mut buffer[start..start + LAYOUT_LEN + trailing_len];
    let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
    let mut writer = writer.into_stake(meta, stake).unwrap();

    // Mutate via the returned writer's view_mut
    let view = writer.view_mut().unwrap();
    let StakeStateV2ViewMut::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };
    meta.rent_exempt_reserve.set(424242);
    stake.credits_observed.set(7777);

    // Trailing bytes must remain untouched
    assert_eq!(
        &buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
        trailing_before.as_slice()
    );

    let layout_bytes = &buffer[start..start + LAYOUT_LEN];

    // Tail should have been zeroed by Initialized -> Stake and view_mut should not change it
    assert_tail_zeroed(layout_bytes);

    let view = StakeStateV2::from_bytes(layout_bytes).unwrap();
    let StakeStateV2View::Stake {
        meta: view_meta,
        stake: view_stake,
    } = view
    else {
        panic!("expected Stake");
    };
    assert_eq!(view_meta.rent_exempt_reserve.get(), 424242);
    assert_eq!(view_stake.credits_observed.get(), 7777);
}

#[test]
fn chained_transitions_uninitialized_to_initialized_to_stake() {
    let meta1 = Meta {
        rent_exempt_reserve: PodU64::from_primitive(4),
        authorized: Authorized {
            staker: PodAddress::from_bytes([1; 32]),
            withdrawer: PodAddress::from_bytes([2; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(-5),
            epoch: PodU64::from_primitive(6),
            custodian: PodAddress::from_bytes([3; 32]),
        },
    };
    let meta2 = Meta {
        rent_exempt_reserve: PodU64::from_primitive(10),
        authorized: Authorized {
            staker: PodAddress::from_bytes([7; 32]),
            withdrawer: PodAddress::from_bytes([8; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(-11),
            epoch: PodU64::from_primitive(12),
            custodian: PodAddress::from_bytes([9; 32]),
        },
    };
    let warmup_rate: f64 = 0.75;
    let stake2 = Stake {
        delegation: Delegation {
            voter_pubkey: PodAddress::from_bytes([13; 32]),
            stake: PodU64::from_primitive(14),
            activation_epoch: PodU64::from_primitive(15),
            deactivation_epoch: PodU64::from_primitive(16),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(17),
    };

    let start = 1;
    let trailing_len = 32usize;
    let end = start + LAYOUT_LEN + trailing_len;
    let mut buffer = vec![0xAB; start + LAYOUT_LEN + trailing_len];
    buffer[start + STAKE_OFF..start + FLAGS_OFF].fill(0xCD);
    buffer[start + FLAGS_OFF..start + LAYOUT_LEN].copy_from_slice(&[1, 2, 3, 4]);

    write_tag(&mut buffer[start..end], StakeStateV2Tag::Uninitialized);

    buffer[start + LAYOUT_LEN..end].fill(0x7E);
    let trailing_before = buffer[start + LAYOUT_LEN..end].to_vec();

    let slice = &mut buffer[start..end];
    let writer = StakeStateV2::from_bytes_mut(slice)
        .unwrap()
        .into_initialized(meta1)
        .unwrap()
        .into_stake(meta2, stake2)
        .unwrap();

    let view = writer.view().unwrap();
    let StakeStateV2View::Stake { meta, stake } = view else {
        panic!("expected Stake");
    };

    let expected_legacy_meta2 = LegacyMeta {
        rent_exempt_reserve: 10,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([7; 32]),
            withdrawer: Pubkey::new_from_array([8; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: -11,
            epoch: 12,
            custodian: Pubkey::new_from_array([9; 32]),
        },
    };
    let expected_legacy_stake2 = LegacyStake {
        delegation: LegacyDelegation {
            voter_pubkey: Pubkey::new_from_array([13; 32]),
            stake: 14,
            activation_epoch: 15,
            deactivation_epoch: 16,
            warmup_cooldown_rate: warmup_rate,
        },
        credits_observed: 17,
    };
    assert_meta_compat(meta, &expected_legacy_meta2);
    assert_stake_compat(stake, &expected_legacy_stake2);

    assert_eq!(&buffer[start + LAYOUT_LEN..end], trailing_before.as_slice());

    let layout_bytes = &buffer[start..start + LAYOUT_LEN];
    assert_tail_zeroed(layout_bytes);

    let layout = StakeStateV2Layout::from_bytes(layout_bytes).unwrap();
    assert_eq!(layout.tag.get(), StakeStateV2Tag::Stake as u32);
    assert_eq!(layout.meta, meta2);
    assert_eq!(layout.stake, stake2);
    assert_eq!(layout.stake_flags, 0);
    assert_eq!(layout.padding, [0u8; 3]);
}

#[test_case(false, 0; "aligned_no_trailing")]
#[test_case(false, 64; "aligned_trailing")]
#[test_case(true, 0; "unaligned_no_trailing")]
#[test_case(true, 64; "unaligned_trailing")]
fn stake_to_stake_preserves_tail(unaligned: bool, trailing_len: usize) {
    let preserved_flags = 1;
    let preserved_padding = [222, 173, 190];

    let mut base = empty_state_bytes(StakeStateV2Tag::Stake);
    base[FLAGS_OFF] = preserved_flags;
    base[PADDING_OFF..LAYOUT_LEN].copy_from_slice(&preserved_padding);

    let start = if unaligned { 1 } else { 0 };
    let mut buffer = vec![238u8; start + LAYOUT_LEN + trailing_len];
    buffer[start..start + LAYOUT_LEN].copy_from_slice(&base);
    buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(126);

    let expected_trailing = buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(1),
        authorized: Authorized {
            staker: PodAddress::from_bytes([9; 32]),
            withdrawer: PodAddress::from_bytes([8; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(7),
            epoch: PodU64::from_primitive(11),
            custodian: PodAddress::from_bytes([7; 32]),
        },
    };
    let warmup_rate: f64 = 0.5;
    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: PodAddress::from_bytes([6; 32]),
            stake: PodU64::from_primitive(55),
            activation_epoch: PodU64::from_primitive(1),
            deactivation_epoch: PodU64::from_primitive(9),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(99),
    };

    let slice = &mut buffer[start..start + LAYOUT_LEN + trailing_len];
    let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
    writer.into_stake(meta, stake).unwrap();

    assert_eq!(
        &buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
        expected_trailing.as_slice()
    );

    let layout_bytes = &buffer[start..start + LAYOUT_LEN];
    assert_eq!(
        StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
        StakeStateV2Tag::Stake
    );

    assert_tail(layout_bytes, preserved_flags, preserved_padding);

    let old = deserialize_legacy(layout_bytes);
    let LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, legacy_flags) = old else {
        panic!("expected legacy Stake");
    };
    assert_eq!(
        legacy_meta,
        LegacyMeta {
            rent_exempt_reserve: 1,
            authorized: LegacyAuthorized {
                staker: Pubkey::new_from_array([9; 32]),
                withdrawer: Pubkey::new_from_array([8; 32]),
            },
            lockup: LegacyLockup {
                unix_timestamp: 7,
                epoch: 11,
                custodian: Pubkey::new_from_array([7; 32]),
            },
        }
    );
    assert_eq!(
        legacy_stake,
        LegacyStake {
            delegation: LegacyDelegation {
                voter_pubkey: Pubkey::new_from_array([6; 32]),
                stake: 55,
                activation_epoch: 1,
                deactivation_epoch: 9,
                warmup_cooldown_rate: warmup_rate,
            },
            credits_observed: 99,
        }
    );
    let expected_flags = LegacyStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
    assert_eq!(legacy_flags, expected_flags);
}

// ----------------------------- property tests --------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_uninitialized_to_initialized_zeroes_stake_and_tail(
        legacy_meta in arb_legacy_meta(),
        unaligned in any::<bool>(),
        trailing_len in 0usize..64usize,
        fill in any::<u8>(),
    ) {
        let meta = meta_from_legacy(&legacy_meta);

        let start = if unaligned { 1 } else { 0 };
        let end = start + LAYOUT_LEN + trailing_len;
        let mut buffer = vec![fill; start + LAYOUT_LEN + trailing_len];
        buffer[start + STAKE_OFF..start + FLAGS_OFF].fill(0xAB);
        buffer[start + FLAGS_OFF..start + LAYOUT_LEN].copy_from_slice(&[1, 2, 3, 4]);

        write_tag(&mut buffer[start..end], StakeStateV2Tag::Uninitialized);

        buffer[start + LAYOUT_LEN..end].fill(0x7E);
        let trailing_before = buffer[start + LAYOUT_LEN..end].to_vec();

        let slice = &mut buffer[start..end];
        let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
        writer.into_initialized(meta).unwrap();

        prop_assert_eq!(&buffer[start + LAYOUT_LEN..end], trailing_before.as_slice());

        let layout_bytes = &buffer[start..start + LAYOUT_LEN];
        prop_assert_eq!(
            StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
            StakeStateV2Tag::Initialized
        );
        prop_assert!(layout_bytes[STAKE_OFF..FLAGS_OFF].iter().all(|b| *b == 0));
        prop_assert_eq!(layout_bytes[FLAGS_OFF], 0);
        prop_assert_eq!(&layout_bytes[PADDING_OFF..LAYOUT_LEN], &[0u8; 3]);

        let layout = StakeStateV2Layout::from_bytes(layout_bytes).unwrap();
        prop_assert_eq!(layout.tag.get(), StakeStateV2Tag::Initialized as u32);
        prop_assert_eq!(layout.meta, meta);
        prop_assert_eq!(layout.stake, Stake::default());
        prop_assert_eq!(layout.stake_flags, 0);
        prop_assert_eq!(layout.padding, [0u8; 3]);
    }

    // Stake -> Stake transition must preserve arbitrary stake_flags+padding AND preserve trailing bytes beyond 200
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_stake_to_stake_preserves_flags(
        legacy_meta in arb_legacy_meta(),
        legacy_stake in arb_legacy_stake(),
        new_meta in arb_legacy_meta(),
        new_legacy_stake in arb_legacy_stake(),
        arbitrary_flags in any::<u8>(),
        arbitrary_padding in any::<[u8; 3]>(),
        unaligned in any::<bool>(),
        trailing_len in 0usize..64usize,
    ) {
        let new_reserved = warmup_reserved_bytes_from_legacy_rate(new_legacy_stake.delegation.warmup_cooldown_rate);

        let legacy_state = LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, LegacyStakeFlags::empty());
        let base = serialize_legacy(&legacy_state);
        prop_assert_eq!(base.len(), 200);

        let start = if unaligned { 1 } else { 0 };
        let mut buffer = vec![238u8; start + LAYOUT_LEN + trailing_len];
        buffer[start..start + LAYOUT_LEN].copy_from_slice(&base);
        buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(126);

        buffer[start + FLAGS_OFF] = arbitrary_flags;
        buffer[start + PADDING_OFF..start + LAYOUT_LEN].copy_from_slice(&arbitrary_padding);

        let trailing_before = buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

        let meta = meta_from_legacy(&new_meta);
        let stake = Stake {
             delegation: Delegation {
                voter_pubkey: PodAddress::from_bytes(new_legacy_stake.delegation.voter_pubkey.to_bytes()),
                stake: PodU64::from_primitive(new_legacy_stake.delegation.stake),
                activation_epoch: PodU64::from_primitive(new_legacy_stake.delegation.activation_epoch),
                deactivation_epoch: PodU64::from_primitive(new_legacy_stake.delegation.deactivation_epoch),
                _reserved: new_reserved,
            },
            credits_observed: PodU64::from_primitive(new_legacy_stake.credits_observed),
        };

        let slice = &mut buffer[start..start + LAYOUT_LEN + trailing_len];
        let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
        writer.into_stake(meta, stake).unwrap();

        prop_assert_eq!(
            &buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
            trailing_before.as_slice()
        );

        prop_assert_eq!(buffer[start + FLAGS_OFF], arbitrary_flags);
        prop_assert_eq!(
            &buffer[start + PADDING_OFF..start + LAYOUT_LEN],
            arbitrary_padding.as_slice()
        );

        let view = StakeStateV2::from_bytes(&buffer[start..start + LAYOUT_LEN]).unwrap();
        let StakeStateV2View::Stake { meta: view_meta, stake: view_stake } = view else {
            prop_assert!(false, "expected Stake after into_stake");
            return Ok(());
        };

        // Verify that the written fields match the inputs
        assert_meta_compat(view_meta, &new_meta);
        assert_stake_compat(view_stake, &new_legacy_stake);

        let decoded = deserialize_legacy(&buffer[start..start + LAYOUT_LEN]);
        let LegacyStakeStateV2::Stake(decoded_meta, decoded_stake, decoded_flags) = decoded else {
            prop_assert!(false, "expected legacy Stake after into_stake");
            return Ok(());
        };

        // Verify legacy decode matches inputs
        prop_assert_eq!(decoded_meta, new_meta);

        prop_assert_eq!(decoded_stake.credits_observed, new_legacy_stake.credits_observed);
        prop_assert_eq!(decoded_stake.delegation.voter_pubkey, new_legacy_stake.delegation.voter_pubkey);
        prop_assert_eq!(decoded_stake.delegation.stake, new_legacy_stake.delegation.stake);
        prop_assert_eq!(decoded_stake.delegation.activation_epoch, new_legacy_stake.delegation.activation_epoch);
        prop_assert_eq!(decoded_stake.delegation.deactivation_epoch, new_legacy_stake.delegation.deactivation_epoch);

        let decoded_bits = decoded_stake.delegation.warmup_cooldown_rate.to_bits();
        let expected_bits = new_legacy_stake.delegation.warmup_cooldown_rate.to_bits();
        prop_assert_eq!(decoded_bits, expected_bits);

        prop_assert_eq!(stake_flags_byte(&decoded_flags), arbitrary_flags);
    }

    // Initialized -> Stake transition must always zero out flag/padding bytes
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_initialized_to_stake_zeroes_tail(
        legacy_meta in arb_legacy_meta(),
        new_meta in arb_legacy_meta(),
        new_legacy_stake in arb_legacy_stake(),
        mut dirty_tail in any::<[u8; 4]>(),
        unaligned in any::<bool>(),
        trailing_len in 0usize..64usize,
    ) {
        let legacy_state = LegacyStakeStateV2::Initialized(legacy_meta);
        let base = serialize_legacy(&legacy_state);
        prop_assert_eq!(base.len(), 200);

        let start = if unaligned { 1 } else { 0 };
        let mut buffer = vec![238u8; start + LAYOUT_LEN + trailing_len];
        buffer[start..start + LAYOUT_LEN].copy_from_slice(&base);
        buffer[start + LAYOUT_LEN..].fill(126);

        // Corrupt the tail region (flags + padding) to ensure it gets cleared
        // Make sure it's actually non-zero for the test to be meaningful
        if dirty_tail == [0, 0, 0, 0] {
            dirty_tail = [1, 2, 3, 4];
        }
        buffer[start + FLAGS_OFF..start + LAYOUT_LEN].copy_from_slice(&dirty_tail);

        let trailing_before = buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

        let new_reserved = warmup_reserved_bytes_from_legacy_rate(new_legacy_stake.delegation.warmup_cooldown_rate);

        let meta = meta_from_legacy(&new_meta);
        let stake = Stake {
             delegation: Delegation {
                voter_pubkey: PodAddress::from_bytes(new_legacy_stake.delegation.voter_pubkey.to_bytes()),
                stake: PodU64::from_primitive(new_legacy_stake.delegation.stake),
                activation_epoch: PodU64::from_primitive(new_legacy_stake.delegation.activation_epoch),
                deactivation_epoch: PodU64::from_primitive(new_legacy_stake.delegation.deactivation_epoch),
                _reserved: new_reserved,
            },
            credits_observed: PodU64::from_primitive(new_legacy_stake.credits_observed),
        };

        let slice = &mut buffer[start..start + LAYOUT_LEN + trailing_len];
        let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
        writer.into_stake(meta, stake).unwrap();

        prop_assert_eq!(
            &buffer[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
            trailing_before.as_slice()
        );

        // Verify tail is zeroed
        prop_assert_eq!(buffer[start + FLAGS_OFF], 0);
        prop_assert_eq!(&buffer[start + PADDING_OFF..start + LAYOUT_LEN], &[0u8; 3]);

        // Verify the rest of the data is correct
        let view = StakeStateV2::from_bytes(&buffer[start..start + LAYOUT_LEN]).unwrap();
        let StakeStateV2View::Stake { meta: view_meta, stake: view_stake } = view else {
             prop_assert!(false, "expected Stake after into_stake");
             return Ok(());
        };

        assert_meta_compat(view_meta, &new_meta);
        assert_stake_compat(view_stake, &new_legacy_stake);
    }
}
