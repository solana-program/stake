mod common;

use {
    bincode::Options,
    common::*,
    core::mem::size_of,
    p_stake_interface::{
        error::StakeStateError,
        state::{
            StakeStateV2, StakeStateV2Layout, StakeStateV2Tag, StakeStateV2View,
            StakeStateV2ViewMut,
        },
    },
    proptest::prelude::*,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as OldStakeFlags,
        state::{
            Authorized as OldAuthorized, Delegation as OldDelegation, Lockup as OldLockup,
            Meta as OldMeta, Stake as OldStake, StakeStateV2 as OldStakeStateV2,
        },
    },
};

/// Bundles a legacy state plus any extra “byte-level” expectations we care about.
#[derive(Clone, Debug)]
struct LegacyCase {
    old_state: OldStakeStateV2,
    /// For Stake variant: the exact 8 bytes that legacy bincode stored for warmup_cooldown_rate.
    delegation_reserved_bytes: Option<[u8; 8]>,
    /// For Stake variant: the exact raw stake_flags byte expected at offset 196.
    stake_flags_byte: Option<u8>,
}

fn arb_old_meta() -> impl Strategy<Value = OldMeta> {
    (
        any::<u64>(),
        any::<[u8; 32]>(),
        any::<[u8; 32]>(),
        any::<i64>(),
        any::<u64>(),
        any::<[u8; 32]>(),
    )
        .prop_map(
            |(
                rent_exempt_reserve,
                staker_bytes,
                withdrawer_bytes,
                lockup_timestamp,
                lockup_epoch,
                custodian_bytes,
            )| OldMeta {
                rent_exempt_reserve,
                authorized: OldAuthorized {
                    staker: Pubkey::new_from_array(staker_bytes),
                    withdrawer: Pubkey::new_from_array(withdrawer_bytes),
                },
                lockup: OldLockup {
                    unix_timestamp: lockup_timestamp,
                    epoch: lockup_epoch,
                    custodian: Pubkey::new_from_array(custodian_bytes),
                },
            },
        )
}

fn arb_old_stake() -> impl Strategy<Value = (OldStake, [u8; 8])> {
    (
        any::<[u8; 32]>(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        any::<[u8; 8]>(),
        any::<u64>(),
    )
        .prop_map(
            |(
                voter_bytes,
                stake_amount,
                activation_epoch,
                deactivation_epoch,
                reserved_bytes,
                credits_observed,
            )| {
                #[allow(deprecated)]
                let old_delegation = OldDelegation {
                    voter_pubkey: Pubkey::new_from_array(voter_bytes),
                    stake: stake_amount,
                    activation_epoch,
                    deactivation_epoch,
                    warmup_cooldown_rate: f64::from_le_bytes(reserved_bytes),
                };

                let old_stake = OldStake {
                    delegation: old_delegation,
                    credits_observed,
                };

                (old_stake, reserved_bytes)
            },
        )
}

fn arb_legacy_case() -> impl Strategy<Value = LegacyCase> {
    prop_oneof![
        Just(LegacyCase {
            old_state: OldStakeStateV2::Uninitialized,
            delegation_reserved_bytes: None,
            stake_flags_byte: None,
        }),
        arb_old_meta().prop_map(|meta| LegacyCase {
            old_state: OldStakeStateV2::Initialized(meta),
            delegation_reserved_bytes: None,
            stake_flags_byte: None,
        }),
        (arb_old_meta(), arb_old_stake(), any::<bool>()).prop_map(
            |(meta, (stake, reserved), flag_set)| {
                let mut old_flags = OldStakeFlags::empty();
                #[allow(deprecated)]
                if flag_set {
                    old_flags
                        .set(OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
                }
                // Serialize old_flags to get the raw bits byte (bits is a private field)
                let flags_byte = bincode_opts().serialize(&old_flags).unwrap()[0];

                LegacyCase {
                    old_state: OldStakeStateV2::Stake(meta, stake, old_flags),
                    delegation_reserved_bytes: Some(reserved),
                    stake_flags_byte: Some(flags_byte),
                }
            }
        ),
        Just(LegacyCase {
            old_state: OldStakeStateV2::RewardsPool,
            delegation_reserved_bytes: None,
            stake_flags_byte: None,
        }),
    ]
}

fn arb_valid_tag() -> impl Strategy<Value = StakeStateV2Tag> {
    prop_oneof![
        Just(StakeStateV2Tag::Uninitialized),
        Just(StakeStateV2Tag::Initialized),
        Just(StakeStateV2Tag::Stake),
        Just(StakeStateV2Tag::RewardsPool),
    ]
}

fn write_tag(bytes: &mut [u8], tag: StakeStateV2Tag) {
    let mut slice = &mut bytes[..StakeStateV2Tag::TAG_LEN];
    wincode::serialize_into(&mut slice, &tag).unwrap();
}

fn is_in_ranges(i: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|(start, end)| i >= *start && i < *end)
}

proptest! {
    #[test]
    fn given_random_legacy_variants_when_view_then_matches_bincode(case in arb_legacy_case()) {
        let data = serialize_old(&case.old_state);
        assert_eq!(data.len(), size_of::<StakeStateV2Layout>());
        assert_eq!(data.len(), 200);

        let view = StakeStateV2::from_bytes(&data).unwrap();

        match (&case.old_state, view) {
            (OldStakeStateV2::Uninitialized, StakeStateV2View::Uninitialized) => {
                // legacy serialization resized to 200 => tail should be zero
                prop_assert_eq!(data[196], 0);
                prop_assert_eq!(&data[197..200], &[0u8; 3]);
            }
            (OldStakeStateV2::RewardsPool, StakeStateV2View::RewardsPool) => {
                prop_assert_eq!(data[196], 0);
                prop_assert_eq!(&data[197..200], &[0u8; 3]);
            }
            (OldStakeStateV2::Initialized(old_meta), StakeStateV2View::Initialized(meta)) => {
                prop_assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
                prop_assert_eq!(
                    Pubkey::new_from_array(*meta.authorized.staker.as_bytes()),
                    old_meta.authorized.staker
                );
                prop_assert_eq!(
                    Pubkey::new_from_array(*meta.authorized.withdrawer.as_bytes()),
                    old_meta.authorized.withdrawer
                );
                prop_assert_eq!(meta.lockup.unix_timestamp.get(), old_meta.lockup.unix_timestamp);
                prop_assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
                prop_assert_eq!(
                    Pubkey::new_from_array(*meta.lockup.custodian.as_bytes()),
                    old_meta.lockup.custodian
                );

                // legacy serialization resized to 200 => tail should be zero
                prop_assert_eq!(data[196], 0);
                prop_assert_eq!(&data[197..200], &[0u8; 3]);
            }
            (OldStakeStateV2::Stake(old_meta, old_stake, old_flags), StakeStateV2View::Stake { meta, stake }) => {
                prop_assert_eq!(meta.rent_exempt_reserve.get(), old_meta.rent_exempt_reserve);
                prop_assert_eq!(
                    Pubkey::new_from_array(*meta.authorized.staker.as_bytes()),
                    old_meta.authorized.staker
                );
                prop_assert_eq!(
                    Pubkey::new_from_array(*meta.authorized.withdrawer.as_bytes()),
                    old_meta.authorized.withdrawer
                );
                prop_assert_eq!(meta.lockup.unix_timestamp.get(), old_meta.lockup.unix_timestamp);
                prop_assert_eq!(meta.lockup.epoch.get(), old_meta.lockup.epoch);
                prop_assert_eq!(
                    Pubkey::new_from_array(*meta.lockup.custodian.as_bytes()),
                    old_meta.lockup.custodian
                );

                prop_assert_eq!(
                    Pubkey::new_from_array(*stake.delegation.voter_pubkey.as_bytes()),
                    old_stake.delegation.voter_pubkey
                );
                prop_assert_eq!(stake.delegation.stake.get(), old_stake.delegation.stake);
                prop_assert_eq!(stake.delegation.activation_epoch.get(), old_stake.delegation.activation_epoch);
                prop_assert_eq!(stake.delegation.deactivation_epoch.get(), old_stake.delegation.deactivation_epoch);

                // Legacy f64 bytes must roundtrip exactly into the reserved bytes region.
                let expected_reserved = case.delegation_reserved_bytes.expect("stake must have reserved bytes");
                prop_assert_eq!(stake.delegation._reserved, expected_reserved);

                prop_assert_eq!(stake.credits_observed.get(), old_stake.credits_observed);

                // The raw flags byte must match legacy bits (and be at the known offset).
                let expected_flags_byte = case.stake_flags_byte.expect("stake must have flags byte");
                // Serialize old_flags to get the raw bits byte (bits is a private field)
                let actual_flags_byte = bincode_opts().serialize(old_flags).unwrap()[0];
                prop_assert_eq!(expected_flags_byte, actual_flags_byte);
                prop_assert_eq!(data[196], expected_flags_byte);
            }
            _ => prop_assert!(false, "unexpected (legacy, view) pairing"),
        }
    }

    #[test]
    fn given_random_stake_when_view_mut_updates_then_only_expected_bytes_change_and_tail_preserved(
        old_meta in arb_old_meta(),
        old_stake_and_reserved in arb_old_stake(),
        // raw tail bytes to prove we preserve unknown bits/padding
        raw_flags in any::<u8>(),
        raw_padding in any::<[u8; 3]>(),

        // new values (the mutations we perform)
        new_rent_exempt_reserve in any::<u64>(),
        new_credits_observed in any::<u64>(),
        new_stake_amount in any::<u64>(),
    ) {
        let (old_stake, _reserved_bytes) = old_stake_and_reserved;

        // Start with a legacy Stake state, then overwrite tail bytes to arbitrary values.
        let old_state = OldStakeStateV2::Stake(old_meta, old_stake, OldStakeFlags::empty());
        let mut data = serialize_old(&old_state);
        assert_eq!(data.len(), size_of::<StakeStateV2Layout>());
        assert_eq!(data.len(), 200);

        data[196] = raw_flags;
        data[197..200].copy_from_slice(&raw_padding);

        let before = data.clone();

        // --- mutate via zero-copy mutable view ---
        {
            let view = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap();
            let StakeStateV2ViewMut::Stake { meta, stake } = view else {
                prop_assert!(false, "expected Stake");
                return Ok(());
            };

            meta.rent_exempt_reserve.set(new_rent_exempt_reserve);
            stake.credits_observed.set(new_credits_observed);
            stake.delegation.stake.set(new_stake_amount);
        }

        // --- byte-level safety: tail must be preserved exactly ---
        prop_assert_eq!(data[196], before[196]);
        prop_assert_eq!(&data[197..200], &before[197..200]);

        // --- byte-level safety: only the three fields we set are allowed to change ---
        // Pre-computed offsets to avoid `arithmetic_side_effects` clippy lint in tests.
        // tag_off = StakeStateV2Tag::TAG_LEN = 4   (for u32 tag)
        // meta size = 120 bytes, stake_off = delegation_off = 4 + 0 = 4 (relative to Meta start at 4)
        // For clarity: StakeStateV2Tag::TAG_LEN (4) + Meta size (120) = 124
        // But stake/delegation is at offset 4 + 120 = 124
        // delegation_stake_off = 124 + 32 = 156 (voter_pubkey is 32 bytes)
        // credits_off = 124 + size_of::<Delegation>() = 124 + 64 = 188
        const TAG_OFF: usize = 4; // StakeStateV2Tag::TAG_LEN
        const META_SIZE: usize = 120; // size_of::<Meta>()
        const DELEGATION_SIZE: usize = 64; // size_of::<Delegation>()
        const STAKE_OFF: usize = TAG_OFF + META_SIZE; // 124
        const DELEGATION_OFF: usize = STAKE_OFF; // Stake starts with Delegation
        const DELEGATION_STAKE_OFF: usize = DELEGATION_OFF + 32; // voter_pubkey is 32 bytes = 156
        const CREDITS_OFF: usize = DELEGATION_OFF + DELEGATION_SIZE; // 188

        let allowed_ranges = [
            // meta.rent_exempt_reserve (at TAG_OFF, 8 bytes)
            (TAG_OFF, TAG_OFF + 8),
            // stake.delegation.stake (at DELEGATION_STAKE_OFF, 8 bytes)
            (DELEGATION_STAKE_OFF, DELEGATION_STAKE_OFF + 8),
            // stake.credits_observed (at CREDITS_OFF, 8 bytes)
            (CREDITS_OFF, CREDITS_OFF + 8),
        ];

        for i in 0..200 {
            if is_in_ranges(i, &allowed_ranges) {
                continue;
            }
            let msg = format!("unexpected byte change at index {}", i);
            prop_assert_eq!(
                data[i], before[i],
                "{}", msg
            );
        }

        // --- semantic checks via zero-copy view ---
        let view = StakeStateV2::from_bytes(&data).unwrap();
        let StakeStateV2View::Stake { meta, stake, .. } = view else {
            prop_assert!(false, "expected Stake");
            return Ok(());
        };
        prop_assert_eq!(meta.rent_exempt_reserve.get(), new_rent_exempt_reserve);
        prop_assert_eq!(stake.credits_observed.get(), new_credits_observed);
        prop_assert_eq!(stake.delegation.stake.get(), new_stake_amount);

        // --- bincode compatibility + "everything else unchanged" (including warmup bytes) ---
        let decoded_after = deserialize_old(&data);
        let decoded_before = deserialize_old(&before);

        let (OldStakeStateV2::Stake(meta_a, stake_a, flags_a), OldStakeStateV2::Stake(meta_b, stake_b, flags_b)) =
            (decoded_after, decoded_before)
        else {
            prop_assert!(false, "expected Stake (bincode)");
            return Ok(());
        };

        // Flags preserved (byte-level already checked, this is extra)
        prop_assert_eq!(flags_a, flags_b);

        // Unchanged meta fields
        prop_assert_eq!(meta_a.authorized.staker, meta_b.authorized.staker);
        prop_assert_eq!(meta_a.authorized.withdrawer, meta_b.authorized.withdrawer);
        prop_assert_eq!(meta_a.lockup.unix_timestamp, meta_b.lockup.unix_timestamp);
        prop_assert_eq!(meta_a.lockup.epoch, meta_b.lockup.epoch);
        prop_assert_eq!(meta_a.lockup.custodian, meta_b.lockup.custodian);

        // Unchanged delegation fields except stake amount
        prop_assert_eq!(stake_a.delegation.voter_pubkey, stake_b.delegation.voter_pubkey);
        prop_assert_eq!(stake_a.delegation.activation_epoch, stake_b.delegation.activation_epoch);
        prop_assert_eq!(stake_a.delegation.deactivation_epoch, stake_b.delegation.deactivation_epoch);

        // Warmup cooldown rate must be preserved *byte-for-byte*
        #[allow(deprecated)]
        let warmup_a = stake_a.delegation.warmup_cooldown_rate.to_le_bytes();
        #[allow(deprecated)]
        let warmup_b = stake_b.delegation.warmup_cooldown_rate.to_le_bytes();
        prop_assert_eq!(warmup_a, warmup_b);

        // Changed fields
        prop_assert_eq!(meta_a.rent_exempt_reserve, new_rent_exempt_reserve);
        prop_assert_eq!(stake_a.credits_observed, new_credits_observed);
        prop_assert_eq!(stake_a.delegation.stake, new_stake_amount);
    }

    #[test]
    fn given_any_short_buffer_when_view_then_unexpected_eof(data in proptest::collection::vec(any::<u8>(), 0..200)) {
        let err = StakeStateV2::from_bytes(&data).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::UnexpectedEof));
    }

    #[test]
    fn given_any_short_buffer_when_view_mut_then_unexpected_eof(mut data in proptest::collection::vec(any::<u8>(), 0..200)) {
        let err = StakeStateV2ViewMut::from_bytes_mut(&mut data).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::UnexpectedEof));
    }

    #[test]
    fn given_any_200_bytes_with_valid_tag_when_view_then_variant_matches(
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
    fn given_any_200_bytes_with_valid_tag_when_view_mut_then_variant_matches(
        mut bytes in any::<[u8; 200]>(),
        tag in arb_valid_tag(),
    ) {
        write_tag(&mut bytes, tag);

        let view = StakeStateV2ViewMut::from_bytes_mut(&mut bytes).unwrap();
        match (tag, view) {
            (StakeStateV2Tag::Uninitialized, StakeStateV2ViewMut::Uninitialized) => {}
            (StakeStateV2Tag::Initialized, StakeStateV2ViewMut::Initialized(_)) => {}
            (StakeStateV2Tag::Stake, StakeStateV2ViewMut::Stake { .. }) => {}
            (StakeStateV2Tag::RewardsPool, StakeStateV2ViewMut::RewardsPool) => {}
            _ => prop_assert!(false, "tag/view_mut mismatch"),
        }
    }

    #[test]
    fn given_invalid_tag_when_view_then_invalid_tag(
        mut bytes in any::<[u8; 200]>(),
        invalid in any::<u32>().prop_filter("tag must be invalid", |x| *x > 3),
    ) {
        bytes[0..4].copy_from_slice(&invalid.to_le_bytes());
        let err = StakeStateV2::from_bytes(&bytes).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::InvalidTag(t) if t == invalid));
    }

    #[test]
    fn given_invalid_tag_when_view_mut_then_invalid_tag(
        mut bytes in any::<[u8; 200]>(),
        invalid in any::<u32>().prop_filter("tag must be invalid", |x| *x > 3),
    ) {
        bytes[0..4].copy_from_slice(&invalid.to_le_bytes());
        let err = StakeStateV2ViewMut::from_bytes_mut(&mut bytes).unwrap_err();
        prop_assert!(matches!(err, StakeStateError::InvalidTag(t) if t == invalid));
    }

    /// Stake -> Stake writer transition must preserve arbitrary stake_flags bits.
    ///
    /// This catches future refactors that accidentally mask unknown future bits.
    /// Unlike the hardcoded test in writer.rs (which tests 1), this tries all
    /// possible u8 values to ensure forward-compatibility with future flag definitions.
    #[test]
    fn given_stake_state_with_arbitrary_flags_when_into_stake_then_flags_preserved(
        old_meta in arb_old_meta(),
        old_stake_and_reserved in arb_old_stake(),
        arbitrary_flags in any::<u8>(),
        arbitrary_padding in any::<[u8; 3]>(),
        // New values for the transition
        new_meta in arb_old_meta(),
        new_stake_and_reserved in arb_old_stake(),
    ) {
        use p_stake_interface::state::{
            Authorized, Delegation, Lockup, Meta, PodAddress, PodI64, PodU64, Stake,
        };

        let (old_stake, _) = old_stake_and_reserved;
        let (new_stake_old, new_reserved) = new_stake_and_reserved;

        // Serialize initial Stake state with empty flags, then poison the tail bytes.
        let old_state = OldStakeStateV2::Stake(old_meta, old_stake, OldStakeFlags::empty());
        let mut data = serialize_old(&old_state);
        assert_eq!(data.len(), size_of::<StakeStateV2Layout>());
        assert_eq!(data.len(), 200);

        // Overwrite tail with arbitrary values - these must survive the transition.
        const FLAGS_OFF: usize = 196;
        const PADDING_OFF: usize = 197;
        data[FLAGS_OFF] = arbitrary_flags;
        data[PADDING_OFF..200].copy_from_slice(&arbitrary_padding);

        let before_flags = data[FLAGS_OFF];
        let before_padding = data[PADDING_OFF..200].to_vec();

        // Build new Meta and Stake for the transition.
        let meta = Meta {
            rent_exempt_reserve: PodU64::from_primitive(new_meta.rent_exempt_reserve),
            authorized: Authorized {
                staker: PodAddress::from_bytes(new_meta.authorized.staker.to_bytes()),
                withdrawer: PodAddress::from_bytes(new_meta.authorized.withdrawer.to_bytes()),
            },
            lockup: Lockup {
                unix_timestamp: PodI64::from_primitive(new_meta.lockup.unix_timestamp),
                epoch: PodU64::from_primitive(new_meta.lockup.epoch),
                custodian: PodAddress::from_bytes(new_meta.lockup.custodian.to_bytes()),
            },
        };

        let stake = Stake {
            delegation: Delegation {
                voter_pubkey: PodAddress::from_bytes(new_stake_old.delegation.voter_pubkey.to_bytes()),
                stake: PodU64::from_primitive(new_stake_old.delegation.stake),
                activation_epoch: PodU64::from_primitive(new_stake_old.delegation.activation_epoch),
                deactivation_epoch: PodU64::from_primitive(new_stake_old.delegation.deactivation_epoch),
                _reserved: new_reserved,
            },
            credits_observed: PodU64::from_primitive(new_stake_old.credits_observed),
        };

        // Perform Stake -> Stake writer transition.
        {
            let writer = StakeStateV2::from_bytes_mut(&mut data).unwrap();
            let _writer = writer.into_stake(meta, stake).unwrap();
        }

        // Assert tail bytes (flags + padding) are preserved exactly.
        prop_assert_eq!(
            data[FLAGS_OFF], before_flags,
            "stake_flags byte must be preserved on Stake -> Stake transition"
        );
        prop_assert_eq!(
            &data[PADDING_OFF..200], before_padding.as_slice(),
            "padding bytes must be preserved on Stake -> Stake transition"
        );
    }
}
