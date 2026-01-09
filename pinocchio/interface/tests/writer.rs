mod common;

use {
    common::*,
    core::mem::size_of,
    p_stake_interface::{
        error::StakeStateError,
        state::{
            Authorized, Delegation, Lockup, Meta, PodAddress, PodI64, PodU64, Stake, StakeStateV2,
            StakeStateV2Layout, StakeStateV2Tag, StakeStateV2View,
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
const STAKE_OFF: usize = TAG_LEN + size_of::<Meta>();
const FLAGS_OFF: usize = TAG_LEN + size_of::<Meta>() + size_of::<Stake>();
const PADDING_OFF: usize = FLAGS_OFF + 1;
const LAYOUT_LEN: usize = size_of::<StakeStateV2Layout>();

fn write_tag(bytes: &mut [u8], tag: StakeStateV2Tag) {
    let mut slice = &mut bytes[..TAG_LEN];
    wincode::serialize_into(&mut slice, &tag).unwrap();
}

fn reserved_bytes_for_warmup_rate(rate: f64) -> [u8; 8] {
    rate.to_bits().to_le_bytes()
}

fn warmup_rate_from_reserved(reserved: [u8; 8]) -> f64 {
    f64::from_bits(u64::from_le_bytes(reserved))
}

fn example_meta(
    staker: u8,
    withdrawer: u8,
    custodian: u8,
    rent: u64,
    unix: i64,
    epoch: u64,
) -> Meta {
    Meta {
        rent_exempt_reserve: PodU64::from_primitive(rent),
        authorized: Authorized {
            staker: PodAddress::from_bytes([staker; 32]),
            withdrawer: PodAddress::from_bytes([withdrawer; 32]),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(unix),
            epoch: PodU64::from_primitive(epoch),
            custodian: PodAddress::from_bytes([custodian; 32]),
        },
    }
}

fn example_old_meta(
    staker: u8,
    withdrawer: u8,
    custodian: u8,
    rent: u64,
    unix: i64,
    epoch: u64,
) -> OldMeta {
    OldMeta {
        rent_exempt_reserve: rent,
        authorized: OldAuthorized {
            staker: Pubkey::new_from_array([staker; 32]),
            withdrawer: Pubkey::new_from_array([withdrawer; 32]),
        },
        lockup: OldLockup {
            unix_timestamp: unix,
            epoch,
            custodian: Pubkey::new_from_array([custodian; 32]),
        },
    }
}

fn example_stake(
    voter: u8,
    stake_amt: u64,
    act_epoch: u64,
    deact_epoch: u64,
    warmup_rate: f64,
    credits: u64,
) -> Stake {
    let reserved = reserved_bytes_for_warmup_rate(warmup_rate);
    Stake {
        delegation: Delegation {
            voter_pubkey: PodAddress::from_bytes([voter; 32]),
            stake: PodU64::from_primitive(stake_amt),
            activation_epoch: PodU64::from_primitive(act_epoch),
            deactivation_epoch: PodU64::from_primitive(deact_epoch),
            _reserved: reserved,
        },
        credits_observed: PodU64::from_primitive(credits),
    }
}

#[allow(deprecated)]
fn example_old_stake(
    voter: u8,
    stake_amt: u64,
    act_epoch: u64,
    deact_epoch: u64,
    warmup_rate: f64,
    credits: u64,
) -> OldStake {
    OldStake {
        delegation: OldDelegation {
            voter_pubkey: Pubkey::new_from_array([voter; 32]),
            stake: stake_amt,
            activation_epoch: act_epoch,
            deactivation_epoch: deact_epoch,
            warmup_cooldown_rate: warmup_rate,
        },
        credits_observed: credits,
    }
}

fn assert_tail_zeroed(layout_bytes: &[u8]) {
    assert_eq!(layout_bytes[FLAGS_OFF], 0);
    assert_eq!(&layout_bytes[PADDING_OFF..LAYOUT_LEN], &[0u8; 3]);
}

fn assert_tail(layout_bytes: &[u8], flags: u8, padding: [u8; 3]) {
    assert_eq!(layout_bytes[FLAGS_OFF], flags);
    assert_eq!(&layout_bytes[PADDING_OFF..LAYOUT_LEN], &padding);
}

#[test]
fn given_short_buffer_when_writer_then_unexpected_eof() {
    let mut data = vec![0u8; LAYOUT_LEN - 1];
    let err = StakeStateV2::from_bytes_mut(&mut data).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test]
fn given_invalid_tag_when_writer_then_invalid_tag() {
    let mut data = [0u8; LAYOUT_LEN];
    data[0..4].copy_from_slice(&999u32.to_le_bytes());
    let err = StakeStateV2::from_bytes_mut(&mut data).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn given_trailing_bytes_when_writer_then_ok_and_trailing_untouched() {
    let mut data = empty_state_bytes(StakeStateV2Tag::Uninitialized).to_vec();
    data.extend_from_slice(&[171; 64]);

    let expected = data.clone();
    let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    let view = writer.view().unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));

    assert_eq!(
        data, expected,
        "from_bytes_mut must not mutate bytes (even with trailing)"
    );
}

#[test]
fn given_unaligned_slice_when_writer_then_ok_and_noop() {
    let mut backing = vec![238u8; LAYOUT_LEN + 1];
    write_tag(&mut backing[1..1 + TAG_LEN], StakeStateV2Tag::Uninitialized);

    let expected = backing.clone();
    let unaligned = &mut backing[1..1 + LAYOUT_LEN];

    let writer = StakeStateV2::from_bytes_mut(unaligned).unwrap();
    let view = writer.view().unwrap();
    assert!(matches!(view, StakeStateV2View::Uninitialized));

    assert_eq!(
        backing, expected,
        "from_bytes_mut must be a no-op on unaligned slices"
    );
}

#[test]
fn given_legacy_bytes_when_from_bytes_mut_then_buffer_unchanged_even_with_nonzero_tail() {
    // Include a Stake variant too, and poison tail/padding to ensure the constructor
    // doesn’t “sanitize” bytes.
    let legacy_initialized = OldStakeStateV2::Initialized(example_old_meta(
        17,
        34,
        51,
        u64::MAX,
        i64::MIN + 1,
        u64::MAX,
    ));

    #[allow(deprecated)]
    let legacy_flags = OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
    let legacy_stake = OldStakeStateV2::Stake(
        example_old_meta(68, 85, 102, 7, -9, 123),
        example_old_stake(119, 999, 1, 2, 0.5, 88),
        legacy_flags,
    );

    let variants = [
        OldStakeStateV2::Uninitialized,
        legacy_initialized,
        legacy_stake,
        OldStakeStateV2::RewardsPool,
    ];

    for old_state in variants {
        let mut data = serialize_old(&old_state);

        // Poison bytes that are frequently (incorrectly) normalized/cleared by “constructors”.
        data[FLAGS_OFF] = 165;
        data[PADDING_OFF..LAYOUT_LEN].copy_from_slice(&[222, 173, 190]);

        // Also poison part of the stake region (safe for all variants because trailing bytes
        // are allowed and bincode ignores them based on the enum variant).
        data[STAKE_OFF..STAKE_OFF + 8].copy_from_slice(&[204; 8]);

        let expected = data.clone();
        let _writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
        assert_eq!(data, expected, "from_bytes_mut should not modify buffer");
    }
}

#[test]
fn given_invalid_transitions_when_transition_attempted_then_error_and_buffer_unchanged() {
    // Table-driven invalid transition coverage; ensure *no partial writes* occur on errors.
    let cases = [
        // into_initialized invalid from:
        (
            StakeStateV2Tag::Initialized,
            "into_initialized",
            StakeStateV2Tag::Initialized,
        ),
        (
            StakeStateV2Tag::Stake,
            "into_initialized",
            StakeStateV2Tag::Initialized,
        ),
        (
            StakeStateV2Tag::RewardsPool,
            "into_initialized",
            StakeStateV2Tag::Initialized,
        ),
        // into_stake invalid from:
        (
            StakeStateV2Tag::Uninitialized,
            "into_stake",
            StakeStateV2Tag::Stake,
        ),
        (
            StakeStateV2Tag::RewardsPool,
            "into_stake",
            StakeStateV2Tag::Stake,
        ),
    ];

    for (from, op, to) in cases {
        let mut data = empty_state_bytes(from).to_vec();
        // Poison tail so we’d notice any accidental mutation.
        data[FLAGS_OFF..LAYOUT_LEN].copy_from_slice(&[250, 251, 252, 253]);
        let expected = data.clone();

        let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
        let err = match op {
            "into_initialized" => writer.into_initialized(Meta::default()).unwrap_err(),
            "into_stake" => writer
                .into_stake(Meta::default(), Stake::default())
                .unwrap_err(),
            _ => panic!("unknown op"),
        };

        assert!(
            matches!(err, StakeStateError::InvalidTransition { from: f, to: t } if f == from && t == to),
            "unexpected error for {from:?} -> {to:?} via {op}: {err:?}"
        );
        assert_eq!(data, expected, "error paths must not mutate the buffer");
    }
}

#[test]
fn given_invalid_transition_on_unaligned_with_trailing_when_transition_attempted_then_no_mutation()
{
    // Extra safety: ensure error paths remain no-op even on unaligned + trailing input.
    let mut base = empty_state_bytes(StakeStateV2Tag::RewardsPool);
    base[FLAGS_OFF..LAYOUT_LEN].copy_from_slice(&[1, 2, 3, 4]);

    let trailing_len = 64;
    let start = 1;
    let mut backing = vec![238u8; start + LAYOUT_LEN + trailing_len];
    backing[start..start + LAYOUT_LEN].copy_from_slice(&base);
    backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(123);

    let expected_layout = backing[start..start + LAYOUT_LEN].to_vec();
    let expected_trailing = backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

    let slice = &mut backing[start..start + LAYOUT_LEN + trailing_len];
    let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
    let err = writer.into_initialized(Meta::default()).unwrap_err();

    assert!(matches!(
        err,
        StakeStateError::InvalidTransition {
            from: StakeStateV2Tag::RewardsPool,
            to: StakeStateV2Tag::Initialized
        }
    ));

    assert_eq!(
        &backing[start..start + LAYOUT_LEN],
        expected_layout.as_slice(),
        "layout bytes must be unchanged on error"
    );
    assert_eq!(
        &backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
        expected_trailing.as_slice(),
        "trailing bytes must be untouched"
    );
}

#[test]
fn given_uninitialized_bytes_when_into_initialized_then_zeroes_stake_and_tail_and_matches_legacy_on_all_shapes(
) {
    assert_eq!(LAYOUT_LEN, 200);

    // Shapes: (unaligned?, trailing_len)
    let shapes = [(false, 0usize), (false, 64), (true, 0), (true, 64)];

    for (unaligned, trailing_len) in shapes {
        let mut base = [170u8; LAYOUT_LEN];
        write_tag(&mut base, StakeStateV2Tag::Uninitialized);

        let start = if unaligned { 1 } else { 0 };
        let mut backing = vec![238u8; start + LAYOUT_LEN + trailing_len];
        backing[start..start + LAYOUT_LEN].copy_from_slice(&base);
        backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(124);

        let expected_trailing =
            backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

        let meta = example_meta(1, 2, 3, 42, -3, 9);
        {
            let slice = &mut backing[start..start + LAYOUT_LEN + trailing_len];
            let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
            let _writer = writer.into_initialized(meta).unwrap();
        }

        // Bytes beyond the 200-byte layout must never be touched.
        assert_eq!(
            &backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
            expected_trailing.as_slice()
        );

        let layout_bytes = &backing[start..start + LAYOUT_LEN];

        assert_eq!(
            StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
            StakeStateV2Tag::Initialized
        );

        // Stake region must be zeroed (we started with 170 everywhere).
        assert!(
            layout_bytes[STAKE_OFF..FLAGS_OFF].iter().all(|b| *b == 0),
            "stake region must be zeroed on Uninitialized -> Initialized"
        );

        // Tail must be zeroed on Uninitialized -> Initialized.
        assert_tail_zeroed(layout_bytes);

        // Verify fields via view API.
        let view = StakeStateV2::from_bytes(layout_bytes).unwrap();
        let StakeStateV2View::Initialized(view_meta) = view else {
            panic!("expected Initialized");
        };
        assert_eq!(view_meta.rent_exempt_reserve.get(), 42);
        assert_eq!(
            Pubkey::new_from_array(*view_meta.authorized.staker.as_bytes()),
            Pubkey::new_from_array([1; 32])
        );
        assert_eq!(
            Pubkey::new_from_array(*view_meta.authorized.withdrawer.as_bytes()),
            Pubkey::new_from_array([2; 32])
        );
        assert_eq!(view_meta.lockup.unix_timestamp.get(), -3);
        assert_eq!(view_meta.lockup.epoch.get(), 9);
        assert_eq!(
            Pubkey::new_from_array(*view_meta.lockup.custodian.as_bytes()),
            Pubkey::new_from_array([3; 32])
        );

        // Cross-check with legacy bincode decoding (independent verification of on-wire bytes).
        let old = deserialize_old(layout_bytes);
        let OldStakeStateV2::Initialized(old_meta) = old else {
            panic!("expected legacy Initialized");
        };
        let expected_old = example_old_meta(1, 2, 3, 42, -3, 9);
        assert_eq!(old_meta, expected_old);
    }
}

#[test]
fn given_initialized_bytes_when_into_stake_then_zeroes_tail_and_matches_legacy_on_all_shapes() {
    assert_eq!(LAYOUT_LEN, 200);

    let shapes = [(false, 0usize), (false, 64), (true, 0), (true, 64)];

    for (unaligned, trailing_len) in shapes {
        let mut base = empty_state_bytes(StakeStateV2Tag::Initialized);
        // Poison tail so we can prove it gets cleared.
        base[FLAGS_OFF..LAYOUT_LEN].copy_from_slice(&[170, 187, 204, 221]);

        let start = if unaligned { 1 } else { 0 };
        let mut backing = vec![238u8; start + LAYOUT_LEN + trailing_len];
        backing[start..start + LAYOUT_LEN].copy_from_slice(&base);
        backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(125);

        let expected_trailing =
            backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

        let meta = example_meta(4, 5, 6, 7, 6, 8);
        let warmup_rate = 1.0;
        let stake = example_stake(7, 123, 2, 3, warmup_rate, 44);

        {
            let slice = &mut backing[start..start + LAYOUT_LEN + trailing_len];
            let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
            let _writer = writer.into_stake(meta, stake).unwrap();
        }

        assert_eq!(
            &backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
            expected_trailing.as_slice()
        );

        let layout_bytes = &backing[start..start + LAYOUT_LEN];
        assert_eq!(
            StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
            StakeStateV2Tag::Stake
        );

        // Tail must be zeroed on Initialized -> Stake.
        assert_tail_zeroed(layout_bytes);

        // Verify fields via view API.
        let view = StakeStateV2::from_bytes(layout_bytes).unwrap();
        let StakeStateV2View::Stake { meta, stake, .. } = view else {
            panic!("expected Stake");
        };
        assert_eq!(meta.rent_exempt_reserve.get(), 7);
        assert_eq!(
            Pubkey::new_from_array(*meta.authorized.staker.as_bytes()),
            Pubkey::new_from_array([4; 32])
        );
        assert_eq!(
            Pubkey::new_from_array(*meta.authorized.withdrawer.as_bytes()),
            Pubkey::new_from_array([5; 32])
        );
        assert_eq!(meta.lockup.unix_timestamp.get(), 6);
        assert_eq!(meta.lockup.epoch.get(), 8);
        assert_eq!(
            Pubkey::new_from_array(*meta.lockup.custodian.as_bytes()),
            Pubkey::new_from_array([6; 32])
        );

        assert_eq!(
            Pubkey::new_from_array(*stake.delegation.voter_pubkey.as_bytes()),
            Pubkey::new_from_array([7; 32])
        );
        assert_eq!(stake.delegation.stake.get(), 123);
        assert_eq!(stake.delegation.activation_epoch.get(), 2);
        assert_eq!(stake.delegation.deactivation_epoch.get(), 3);
        assert_eq!(
            warmup_rate_from_reserved(stake.delegation._reserved),
            warmup_rate
        );
        assert_eq!(stake.credits_observed.get(), 44);

        // Cross-check with legacy bincode decoding.
        let old = deserialize_old(layout_bytes);
        let OldStakeStateV2::Stake(old_meta, old_stake, old_flags) = old else {
            panic!("expected legacy Stake");
        };
        assert_eq!(old_meta, example_old_meta(4, 5, 6, 7, 6, 8));
        assert_eq!(old_stake, example_old_stake(7, 123, 2, 3, warmup_rate, 44));
        assert_eq!(old_flags, OldStakeFlags::empty());
    }
}

#[test]
fn given_stake_bytes_when_into_stake_then_preserves_tail_and_trailing_bytes_and_matches_legacy_on_all_shapes(
) {
    assert_eq!(LAYOUT_LEN, 200);

    // Preserve tail bytes on Stake -> Stake:
    // - stake_flags is preserved (including potentially unknown future bits)
    // - padding is preserved as raw bytes
    let preserved_flags: u8 = 1; // known bit used by legacy constant
    let preserved_padding: [u8; 3] = [222, 173, 190];

    let shapes = [(false, 0usize), (false, 64), (true, 0), (true, 64)];

    for (unaligned, trailing_len) in shapes {
        let mut base = empty_state_bytes(StakeStateV2Tag::Stake);
        base[FLAGS_OFF] = preserved_flags;
        base[PADDING_OFF..LAYOUT_LEN].copy_from_slice(&preserved_padding);

        let start = if unaligned { 1 } else { 0 };
        let mut backing = vec![238u8; start + LAYOUT_LEN + trailing_len];
        backing[start..start + LAYOUT_LEN].copy_from_slice(&base);
        backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].fill(126);

        let expected_trailing =
            backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len].to_vec();

        let meta = example_meta(9, 8, 7, 1, 7, 11);
        let warmup_rate = 0.5;
        let stake = example_stake(6, 55, 1, 9, warmup_rate, 99);

        {
            let slice = &mut backing[start..start + LAYOUT_LEN + trailing_len];
            let writer = StakeStateV2::from_bytes_mut(slice).unwrap();
            let _writer = writer.into_stake(meta, stake).unwrap();
        }

        assert_eq!(
            &backing[start + LAYOUT_LEN..start + LAYOUT_LEN + trailing_len],
            expected_trailing.as_slice()
        );

        let layout_bytes = &backing[start..start + LAYOUT_LEN];
        assert_eq!(
            StakeStateV2Tag::from_bytes(layout_bytes).unwrap(),
            StakeStateV2Tag::Stake
        );

        // Tail must be preserved on Stake -> Stake.
        assert_tail(layout_bytes, preserved_flags, preserved_padding);

        // Cross-check with legacy decoding:
        // - flags must remain set to the legacy constant value
        // - padding is ignored by legacy bincode but we already checked it bytewise
        let old = deserialize_old(layout_bytes);
        let OldStakeStateV2::Stake(old_meta, old_stake, old_flags) = old else {
            panic!("expected legacy Stake");
        };
        assert_eq!(old_meta, example_old_meta(9, 8, 7, 1, 7, 11));
        assert_eq!(old_stake, example_old_stake(6, 55, 1, 9, warmup_rate, 99));
        #[allow(deprecated)]
        let expected_flags = OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
        assert_eq!(old_flags, expected_flags);
    }
}
