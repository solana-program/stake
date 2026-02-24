#![allow(clippy::arithmetic_side_effects)]
#![allow(deprecated)]

mod helpers;

use {
    helpers::*,
    p_stake_interface::{
        error::StakeStateError,
        state::{Authorized, Delegation, Lockup, Meta, Stake, StakeStateV2, StakeStateV2Tag},
    },
    solana_address::Address,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as LegacyStakeFlags,
        state::{
            Authorized as LegacyAuthorized, Delegation as LegacyDelegation, Lockup as LegacyLockup,
            Meta as LegacyMeta, Stake as LegacyStake, StakeStateV2 as LegacyStakeStateV2,
        },
    },
    spl_pod::primitives::{PodI64, PodU64},
    test_case::test_case,
};

/// Creates a test buffer with the given base bytes placed at an optional 1-byte offset
/// (for unaligned access testing) and trailing bytes filled with the given value.
///
/// Returns `(buffer, start)` where `start` is the offset of the layout bytes.
fn test_buffer(
    base: &[u8],
    unaligned: bool,
    trailing_len: usize,
    trailing_fill: u8,
) -> (Vec<u8>, usize) {
    assert_eq!(base.len(), STATE_LEN);
    let start = if unaligned { 1 } else { 0 };
    let mut buffer = vec![238u8; start + STATE_LEN + trailing_len];
    buffer[start..start + STATE_LEN].copy_from_slice(base);
    buffer[start + STATE_LEN..].fill(trailing_fill);
    (buffer, start)
}

#[test]
fn trailing_bytes_untouched() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.extend_from_slice(&[171; 64]);

    let expected = data.clone();

    let layout = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Uninitialized);
    assert_eq!(data, expected);
}

#[test]
fn unaligned_slice_noop() {
    let mut buffer = vec![238u8; STATE_LEN + 1];
    write_tag(&mut buffer[1..1 + TAG_LEN], StakeStateV2Tag::Uninitialized);

    let expected = buffer.clone();
    let unaligned = &mut buffer[1..1 + STATE_LEN];

    let layout = StakeStateV2::from_bytes_mut(unaligned).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Uninitialized);
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

        data[PADDING_OFFSET..STATE_LEN].copy_from_slice(&[165, 222, 173, 190]);
        data[STAKE_OFFSET..STAKE_OFFSET + 8].copy_from_slice(&[204; 8]);

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
        ToInitialized,
        ToStake,
    }

    let tags = [
        StakeStateV2Tag::Uninitialized,
        StakeStateV2Tag::Initialized,
        StakeStateV2Tag::Stake,
        StakeStateV2Tag::RewardsPool,
    ];

    for from in tags {
        for &op in &[Op::ToInitialized, Op::ToStake] {
            let mut base = empty_state_bytes(from);
            base[PADDING_OFFSET..STATE_LEN].copy_from_slice(&[250, 251, 252, 253]);

            let (mut buffer, start) = test_buffer(&base, unaligned, trailing_len, 123);

            let expected_layout = buffer[start..start + STATE_LEN].to_vec();
            let expected_trailing =
                buffer[start + STATE_LEN..start + STATE_LEN + trailing_len].to_vec();

            let result = {
                let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
                let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
                match op {
                    Op::ToInitialized => layout.initialize(Meta::default()),
                    Op::ToStake => layout.delegate(Meta::default(), Stake::default()),
                }
            };

            let allowed = matches!(
                (from, op),
                (StakeStateV2Tag::Uninitialized, Op::ToInitialized)
                    | (StakeStateV2Tag::Initialized, Op::ToStake)
                    | (StakeStateV2Tag::Stake, Op::ToStake)
            );

            if allowed {
                assert!(result.is_ok());
            } else {
                let expected_to = match op {
                    Op::ToInitialized => StakeStateV2Tag::Initialized,
                    Op::ToStake => StakeStateV2Tag::Stake,
                };
                let err = result.unwrap_err();
                assert!(matches!(
                    err,
                    StakeStateError::InvalidTransition { from: f, to: t }
                        if f == from && t == expected_to
                ));
                assert_eq!(
                    &buffer[start..start + STATE_LEN],
                    expected_layout.as_slice()
                );
                assert_eq!(
                    &buffer[start + STATE_LEN..start + STATE_LEN + trailing_len],
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
    let mut base = [170u8; STATE_LEN];
    write_tag(&mut base, StakeStateV2Tag::Uninitialized);

    let (mut buffer, start) = test_buffer(&base, unaligned, trailing_len, 124);

    let expected_trailing = buffer[start + STATE_LEN..start + STATE_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(42),
        authorized: Authorized {
            staker: Address::new_from_array([1; 32]),
            withdrawer: Address::new_from_array([2; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(-3),
            epoch: PodU64::from_primitive(9),
            custodian: Address::new_from_array([3; 32]),
        },
    };

    let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    layout.initialize(meta).unwrap();

    assert_eq!(
        &buffer[start + STATE_LEN..start + STATE_LEN + trailing_len],
        expected_trailing.as_slice()
    );

    let layout_bytes = &buffer[start..start + STATE_LEN];

    assert_eq!(
        StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
        StakeStateV2Tag::Initialized
    );
    assert!(layout_bytes[STAKE_OFFSET..PADDING_OFFSET]
        .iter()
        .all(|b| *b == 0));

    assert_tail_zeroed(layout_bytes);

    let layout = StakeStateV2::from_bytes(layout_bytes).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);

    let view_meta = layout.meta().unwrap();

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
    // Nonzero tail is not a possible scenario, but it is used
    // to assert the state transitions do not modify it.
    let mut base = empty_state_bytes(StakeStateV2Tag::Initialized);
    let preserved_tail: [u8; 4] = [1, 222, 173, 190];
    base[PADDING_OFFSET..STATE_LEN].copy_from_slice(&preserved_tail);

    let (mut buffer, start) = test_buffer(&base, unaligned, trailing_len, 125);

    let expected_trailing = buffer[start + STATE_LEN..start + STATE_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(7),
        authorized: Authorized {
            staker: Address::new_from_array([4; 32]),
            withdrawer: Address::new_from_array([5; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(6),
            epoch: PodU64::from_primitive(8),
            custodian: Address::new_from_array([6; 32]),
        },
    };
    let warmup_rate: f64 = 1.0;
    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: Address::new_from_array([7; 32]),
            stake: PodU64::from_primitive(123),
            activation_epoch: PodU64::from_primitive(2),
            deactivation_epoch: PodU64::from_primitive(3),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(44),
    };

    let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    layout.delegate(meta, stake).unwrap();

    assert_eq!(
        &buffer[start + STATE_LEN..start + STATE_LEN + trailing_len],
        expected_trailing.as_slice()
    );

    let layout_bytes = &buffer[start..start + STATE_LEN];
    assert_eq!(
        StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
        StakeStateV2Tag::Stake
    );

    assert_tail(layout_bytes, preserved_tail);

    let layout = StakeStateV2::from_bytes(layout_bytes).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Stake);

    let view_meta = layout.meta().unwrap();
    let view_stake = layout.stake().unwrap();

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

    assert_meta_compat(view_meta, &expected_legacy);
    assert_stake_compat(view_stake, &expected_legacy_stake);

    let old = deserialize_legacy(layout_bytes);
    let LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, legacy_flags) = old else {
        panic!("expected legacy Stake");
    };
    assert_eq!(legacy_meta, expected_legacy);
    assert_eq!(legacy_stake, expected_legacy_stake);
    assert_eq!(stake_flags_byte(&legacy_flags), preserved_tail[0]);
}

#[test_case(false, 0; "aligned_no_trailing")]
#[test_case(false, 64; "aligned_trailing")]
#[test_case(true, 0; "unaligned_no_trailing")]
#[test_case(true, 64; "unaligned_trailing")]
fn initialized_to_stake_meta_mut_works(unaligned: bool, trailing_len: usize) {
    // Nonzero tail is not a possible scenario, but it is used
    // to assert the state transitions do not modify it.
    let mut base = empty_state_bytes(StakeStateV2Tag::Initialized);
    let preserved_tail: [u8; 4] = [1, 222, 173, 190];
    base[PADDING_OFFSET..STATE_LEN].copy_from_slice(&preserved_tail);

    let (mut buffer, start) = test_buffer(&base, unaligned, trailing_len, 99);

    let trailing_before = buffer[start + STATE_LEN..start + STATE_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(7),
        authorized: Authorized {
            staker: Address::new_from_array([4; 32]),
            withdrawer: Address::new_from_array([5; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(6),
            epoch: PodU64::from_primitive(8),
            custodian: Address::new_from_array([6; 32]),
        },
    };
    let warmup_rate: f64 = 1.0;
    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: Address::new_from_array([7; 32]),
            stake: PodU64::from_primitive(123),
            activation_epoch: PodU64::from_primitive(2),
            deactivation_epoch: PodU64::from_primitive(3),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(44),
    };

    let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    layout.delegate(meta, stake).unwrap();

    // Mutate via the layout's mutable accessors
    let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    layout.meta_mut().unwrap().rent_exempt_reserve = PodU64::from(424242u64);
    layout.stake_mut().unwrap().credits_observed = PodU64::from(7777u64);

    // Trailing bytes must remain untouched
    assert_eq!(
        &buffer[start + STATE_LEN..start + STATE_LEN + trailing_len],
        trailing_before.as_slice()
    );

    let layout_bytes = &buffer[start..start + STATE_LEN];

    // Tail must be preserved through delegate() and mut accessors
    assert_tail(layout_bytes, preserved_tail);

    let layout = StakeStateV2::from_bytes(layout_bytes).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Stake);
    assert_eq!(
        u64::from(layout.meta().unwrap().rent_exempt_reserve),
        424242
    );
    assert_eq!(u64::from(layout.stake().unwrap().credits_observed), 7777);
}

#[test]
fn chained_transitions_uninitialized_to_initialized_to_stake() {
    let meta1 = Meta {
        rent_exempt_reserve: PodU64::from_primitive(4),
        authorized: Authorized {
            staker: Address::new_from_array([1; 32]),
            withdrawer: Address::new_from_array([2; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(-5),
            epoch: PodU64::from_primitive(6),
            custodian: Address::new_from_array([3; 32]),
        },
    };
    let meta2 = Meta {
        rent_exempt_reserve: PodU64::from_primitive(10),
        authorized: Authorized {
            staker: Address::new_from_array([7; 32]),
            withdrawer: Address::new_from_array([8; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(-11),
            epoch: PodU64::from_primitive(12),
            custodian: Address::new_from_array([9; 32]),
        },
    };
    let warmup_rate: f64 = 0.75;
    let stake2 = Stake {
        delegation: Delegation {
            voter_pubkey: Address::new_from_array([13; 32]),
            stake: PodU64::from_primitive(14),
            activation_epoch: PodU64::from_primitive(15),
            deactivation_epoch: PodU64::from_primitive(16),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(17),
    };

    let start = 1;
    let trailing_len = 32usize;
    let end = start + STATE_LEN + trailing_len;
    let mut buffer = vec![0xAB; start + STATE_LEN + trailing_len];
    buffer[start + STAKE_OFFSET..start + PADDING_OFFSET].fill(0xCD);
    buffer[start + PADDING_OFFSET..start + STATE_LEN].copy_from_slice(&[1, 2, 3, 4]);

    write_tag(&mut buffer[start..end], StakeStateV2Tag::Uninitialized);

    buffer[start + STATE_LEN..end].fill(0x7E);
    let trailing_before = buffer[start + STATE_LEN..end].to_vec();

    let slice = &mut buffer[start..end];
    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    layout.initialize(meta1).unwrap();

    let slice = &mut buffer[start..end];
    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    layout.delegate(meta2, stake2).unwrap();

    let layout = StakeStateV2::from_bytes(&buffer[start..start + STATE_LEN]).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Stake);

    let view_meta = layout.meta().unwrap();
    let view_stake = layout.stake().unwrap();

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
    assert_meta_compat(view_meta, &expected_legacy_meta2);
    assert_stake_compat(view_stake, &expected_legacy_stake2);

    assert_eq!(&buffer[start + STATE_LEN..end], trailing_before.as_slice());

    let layout_bytes = &buffer[start..start + STATE_LEN];
    assert_tail_zeroed(layout_bytes);
}

#[test_case(false, 0; "aligned_no_trailing")]
#[test_case(false, 64; "aligned_trailing")]
#[test_case(true, 0; "unaligned_no_trailing")]
#[test_case(true, 64; "unaligned_trailing")]
fn stake_to_stake_preserves_tail(unaligned: bool, trailing_len: usize) {
    // Nonzero tail is not a possible scenario, but it is used
    // to assert the state transitions do not modify it.
    let mut base = empty_state_bytes(StakeStateV2Tag::Stake);
    let preserved_tail: [u8; 4] = [1, 222, 173, 190];
    base[PADDING_OFFSET..STATE_LEN].copy_from_slice(&preserved_tail);

    let (mut buffer, start) = test_buffer(&base, unaligned, trailing_len, 126);

    let expected_trailing = buffer[start + STATE_LEN..start + STATE_LEN + trailing_len].to_vec();

    let meta = Meta {
        rent_exempt_reserve: PodU64::from_primitive(1),
        authorized: Authorized {
            staker: Address::new_from_array([9; 32]),
            withdrawer: Address::new_from_array([8; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(7),
            epoch: PodU64::from_primitive(11),
            custodian: Address::new_from_array([7; 32]),
        },
    };
    let warmup_rate: f64 = 0.5;
    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: Address::new_from_array([6; 32]),
            stake: PodU64::from_primitive(55),
            activation_epoch: PodU64::from_primitive(1),
            deactivation_epoch: PodU64::from_primitive(9),
            _reserved: warmup_rate.to_bits().to_le_bytes(),
        },
        credits_observed: PodU64::from_primitive(99),
    };

    let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    layout.delegate(meta, stake).unwrap();

    assert_eq!(
        &buffer[start + STATE_LEN..start + STATE_LEN + trailing_len],
        expected_trailing.as_slice()
    );

    let layout_bytes = &buffer[start..start + STATE_LEN];
    assert_eq!(
        StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
        StakeStateV2Tag::Stake
    );

    assert_tail(layout_bytes, preserved_tail);

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
    assert_eq!(stake_flags_byte(&legacy_flags), preserved_tail[0]);
}
