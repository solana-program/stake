#![allow(clippy::arithmetic_side_effects)]
#![allow(deprecated)]

mod helpers;

use {
    core::mem::size_of,
    helpers::*,
    p_stake_interface::{
        error::StakeStateError,
        state::{Delegation, StakeStateV2, StakeStateV2Tag},
    },
    proptest::prelude::*,
    solana_address::Address,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as LegacyStakeFlags,
        state::{
            Authorized as LegacyAuthorized, Lockup as LegacyLockup, Meta as LegacyMeta,
            StakeStateV2 as LegacyStakeStateV2,
        },
    },
    spl_pod::primitives::PodU64,
    test_case::test_case,
};

// Verifies that the deserialized mutable view is a true zero-copy borrow into the original
// byte slice at the given offset.
fn assert_mut_borrows_at<T>(borrow: &mut T, base_ptr: *mut u8, offset: usize) {
    let ptr = borrow as *mut T as *mut u8;
    let expected = unsafe { base_ptr.add(offset) };
    assert_eq!(ptr, expected);
}

fn overwrite_tail(bytes: &mut [u8], stake_flags: u8, padding: [u8; 3]) -> [u8; 4] {
    bytes[FLAGS_OFF] = stake_flags;
    bytes[PADDING_OFF..STATE_LEN].copy_from_slice(&padding);
    [stake_flags, padding[0], padding[1], padding[2]]
}

#[test_case(StakeStateV2Tag::Uninitialized)]
#[test_case(StakeStateV2Tag::Initialized)]
#[test_case(StakeStateV2Tag::Stake)]
#[test_case(StakeStateV2Tag::RewardsPool)]
fn variants_match_tag(tag: StakeStateV2Tag) {
    let mut data = empty_state_bytes(tag);
    let base_ptr = data.as_mut_ptr();

    let layout = StakeStateV2::from_bytes_mut(&mut data).unwrap();
    assert_eq!(layout.tag(), tag);

    match tag {
        StakeStateV2Tag::Uninitialized | StakeStateV2Tag::RewardsPool => {
            assert!(matches!(
                layout.meta_mut(),
                Err(StakeStateError::InvalidStateAccess(t)) if t == tag
            ));
            assert!(matches!(
                layout.stake_mut(),
                Err(StakeStateError::InvalidStateAccess(t)) if t == tag
            ));
        }
        StakeStateV2Tag::Initialized => {
            let meta = layout.meta_mut().unwrap();
            assert_mut_borrows_at(meta, base_ptr, META_OFF);
            // Re-parse to check stake_mut fails (need fresh layout due to borrow)
            let layout = StakeStateV2::from_bytes_mut(&mut data).unwrap();
            assert!(matches!(
                layout.stake_mut(),
                Err(StakeStateError::InvalidStateAccess(
                    StakeStateV2Tag::Initialized
                ))
            ));
        }
        StakeStateV2Tag::Stake => {
            let meta = layout.meta_mut().unwrap();
            assert_mut_borrows_at(meta, base_ptr, META_OFF);
            // Re-parse for stake_mut - capture base_ptr before borrowing through layout
            let base_ptr = data.as_mut_ptr();
            let layout = StakeStateV2::from_bytes_mut(&mut data).unwrap();
            let stake = layout.stake_mut().unwrap();
            assert_mut_borrows_at(stake, base_ptr, STAKE_OFF);
        }
    }
}

#[test_case(false; "aligned")]
#[test_case(true; "unaligned")]
fn initialized_updates_preserve_tail(is_unaligned: bool) {
    let legacy_meta = LegacyMeta {
        rent_exempt_reserve: 1,
        authorized: LegacyAuthorized {
            staker: Pubkey::new_from_array([1u8; 32]),
            withdrawer: Pubkey::new_from_array([2u8; 32]),
        },
        lockup: LegacyLockup {
            unix_timestamp: 3,
            epoch: 4,
            custodian: Pubkey::new_from_array([5u8; 32]),
        },
    };
    let legacy_state = LegacyStakeStateV2::Initialized(legacy_meta);
    let aligned = serialize_legacy(&legacy_state);

    let new_rent: u64 = 12302652060662169617;
    let new_custodian = [66u8; 32];

    // Test both aligned and unaligned memory access to ensure POD types handle misalignment
    let offset = if is_unaligned { 1 } else { 0 };
    let mut buffer = vec![0u8; offset + STATE_LEN];
    buffer[offset..offset + STATE_LEN].copy_from_slice(&aligned);
    let tail_before = overwrite_tail(
        &mut buffer[offset..offset + STATE_LEN],
        222,
        [173, 190, 239],
    );

    let slice = &mut buffer[offset..offset + STATE_LEN];
    let base_ptr = slice.as_mut_ptr();

    let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);

    let meta = layout.meta_mut().unwrap();

    // Verify mutable view borrows directly into the buffer
    assert_mut_borrows_at(meta, base_ptr, META_OFF);

    // Mutate fields through the view
    meta.rent_exempt_reserve = PodU64::from(new_rent);
    meta.lockup.custodian = Address::new_from_array(new_custodian);

    // Tail bytes (stake_flags + padding) must be untouched by meta_mut operations
    let layout_bytes = &buffer[offset..offset + STATE_LEN];
    assert_eq!(&layout_bytes[FLAGS_OFF..STATE_LEN], &tail_before);

    // Read-only view validates the updates
    let layout = StakeStateV2::from_bytes(layout_bytes).unwrap();
    assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);
    let meta = layout.meta().unwrap();
    assert_eq!(u64::from(meta.rent_exempt_reserve), new_rent);
    assert_eq!(meta.lockup.custodian.to_bytes(), new_custodian);

    // Legacy bincode decode still works
    let decoded = deserialize_legacy(layout_bytes);
    let LegacyStakeStateV2::Initialized(decoded_meta) = decoded else {
        panic!("expected legacy Initialized");
    };
    assert_eq!(decoded_meta.rent_exempt_reserve, new_rent);
    assert_eq!(
        decoded_meta.lockup.custodian,
        Pubkey::new_from_array(new_custodian)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_unaligned_variant_matches_and_borrows(
        mut buffer in any::<[u8; 201]>(),
        tag in arb_valid_tag(),
    ) {
        // Make an unaligned 200-byte window
        let unaligned = &mut buffer[1..1 + STATE_LEN];
        write_tag(unaligned, tag);
        let base_ptr = unaligned.as_mut_ptr();

        let layout = StakeStateV2::from_bytes_mut(unaligned).unwrap();
        prop_assert_eq!(layout.tag(), tag);

        match tag {
            StakeStateV2Tag::Uninitialized | StakeStateV2Tag::RewardsPool => {
                prop_assert!(layout.meta_mut().is_err());
                prop_assert!(layout.stake_mut().is_err());
            }
            StakeStateV2Tag::Initialized => {
                let meta = layout.meta_mut().unwrap();
                assert_mut_borrows_at(meta, base_ptr, META_OFF);
            }
            StakeStateV2Tag::Stake => {
                let meta = layout.meta_mut().unwrap();
                assert_mut_borrows_at(meta, base_ptr, META_OFF);
                // Re-parse for stake_mut - capture base_ptr before borrowing through layout
                let base_ptr = buffer[1..1 + STATE_LEN].as_mut_ptr();
                let unaligned = &mut buffer[1..1 + STATE_LEN];
                let layout = StakeStateV2::from_bytes_mut(unaligned).unwrap();
                let stake = layout.stake_mut().unwrap();
                assert_mut_borrows_at(stake, base_ptr, STAKE_OFF);
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_borrowless_variants_noop(
        mut base in any::<[u8; 200]>(),
        is_rewards_pool in any::<bool>(),
        unaligned in any::<bool>(),
        trailing_len in 0usize..64usize,
        trailing_byte in any::<u8>(),
    ) {
        let tag = if is_rewards_pool { StakeStateV2Tag::RewardsPool } else { StakeStateV2Tag::Uninitialized };

        // Ensure the tag is valid for parsing.
        write_tag(&mut base, tag);

        let start = if unaligned { 1 } else { 0 };
        let mut buffer = vec![238u8; start + STATE_LEN + trailing_len];
        buffer[start..start + STATE_LEN].copy_from_slice(&base);
        buffer[start + STATE_LEN..start + STATE_LEN + trailing_len].fill(trailing_byte);

        let expected = buffer.clone();

        {
            let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
            let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
            prop_assert_eq!(layout.tag(), tag);
            prop_assert!(layout.meta_mut().is_err());
            prop_assert!(layout.stake_mut().is_err());
        }

        prop_assert_eq!(buffer, expected);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_stake_updates_preserve_untouched_bytes(
        legacy_meta in arb_legacy_meta(),
        legacy_stake in arb_legacy_stake(),
        raw_flags in any::<u8>(),
        raw_padding in any::<[u8; 3]>(),
        new_rent_exempt_reserve in any::<u64>(),
        new_credits_observed in any::<u64>(),
        new_stake_amount in any::<u64>(),
        unaligned in any::<bool>(),
        trailing_len in 0usize..64usize,
    ) {
        let reserved_bytes = warmup_reserved_bytes_from_legacy_rate(legacy_stake.delegation.warmup_cooldown_rate);

        let legacy_state = LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, LegacyStakeFlags::empty());
        let base = serialize_legacy(&legacy_state);
        prop_assert_eq!(base.len(), STATE_LEN);

        let start = if unaligned { 1 } else { 0 };
        let mut buffer = vec![0u8; start + STATE_LEN + trailing_len];
        buffer[start..start + STATE_LEN].copy_from_slice(&base);
        buffer[start + STATE_LEN..].fill(126);

        // Make tail arbitrary and ensure we preserve it
        buffer[start + FLAGS_OFF] = raw_flags;
        buffer[start + PADDING_OFF..start + STATE_LEN].copy_from_slice(&raw_padding);

        let before_layout = buffer[start..start + STATE_LEN].to_vec();
        let trailing_before = buffer[start + STATE_LEN..start + STATE_LEN + trailing_len].to_vec();

        let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
        let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
        prop_assert_eq!(layout.tag(), StakeStateV2Tag::Stake);

        let stake = layout.stake_mut().unwrap();
        // Reserved bytes must not change.
        prop_assert_eq!(stake.delegation._reserved, reserved_bytes);

        // Re-parse to mutate more fields
        let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
        let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
        layout.meta_mut().unwrap().rent_exempt_reserve = PodU64::from(new_rent_exempt_reserve);

        let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
        let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
        layout.stake_mut().unwrap().credits_observed = PodU64::from(new_credits_observed);

        let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
        let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
        layout.stake_mut().unwrap().delegation.stake = PodU64::from(new_stake_amount);

        // Verify reserved bytes still unchanged
        let slice = &mut buffer[start..start + STATE_LEN + trailing_len];
        let layout = StakeStateV2::from_bytes_mut(slice).unwrap();
        prop_assert_eq!(layout.stake_mut().unwrap().delegation._reserved, reserved_bytes);

        // Trailing bytes beyond the 200-byte layout must not be modified
        prop_assert_eq!(
            &buffer[start + STATE_LEN..start + STATE_LEN + trailing_len],
            trailing_before.as_slice()
        );
        // Tail bytes must remain untouched
        prop_assert_eq!(buffer[start + FLAGS_OFF], before_layout[FLAGS_OFF]);
        prop_assert_eq!(
            &buffer[start + PADDING_OFF..start + STATE_LEN],
            &before_layout[PADDING_OFF..STATE_LEN]
        );

        // Only specific byte ranges should have changed
        let allowed_ranges = [
            (META_OFF, META_OFF + 8),
            (STAKE_OFF + 32, STAKE_OFF + 32 + 8),
            (STAKE_OFF + size_of::<Delegation>(), STAKE_OFF + size_of::<Delegation>() + 8),
        ];

        let after_layout = &buffer[start..start + STATE_LEN];

        for i in 0..STATE_LEN {
            if allowed_ranges
                .iter()
                .any(|(start, end)| i >= *start && i < *end)
            {
                continue;
            }
            prop_assert_eq!(after_layout[i], before_layout[i]);
        }

        // Read-only view sees the updates
        let layout = StakeStateV2::from_bytes(after_layout).unwrap();
        prop_assert_eq!(layout.tag(), StakeStateV2Tag::Stake);
        let meta = layout.meta().unwrap();
        let stake = layout.stake().unwrap();
        prop_assert_eq!(u64::from(meta.rent_exempt_reserve), new_rent_exempt_reserve);
        prop_assert_eq!(u64::from(stake.credits_observed), new_credits_observed);
        prop_assert_eq!(u64::from(stake.delegation.stake), new_stake_amount);

        // Legacy decode sees the updates and flags/padding are preserved
        let decoded_after = deserialize_legacy(after_layout);
        let decoded_before = deserialize_legacy(&before_layout);

        let (LegacyStakeStateV2::Stake(_, stake_a, flags_a),
             LegacyStakeStateV2::Stake(_, stake_b, flags_b)) = (decoded_after, decoded_before)
        else {
            prop_assert!(false, "expected legacy Stake");
            return Ok(());
        };

        prop_assert_eq!(flags_a, flags_b);
        let warmup_a = warmup_reserved_bytes_from_legacy_rate(stake_a.delegation.warmup_cooldown_rate);
        let warmup_b = warmup_reserved_bytes_from_legacy_rate(stake_b.delegation.warmup_cooldown_rate);
        prop_assert_eq!(warmup_a, warmup_b);
    }
}
