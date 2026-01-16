mod helpers;

use {
    bincode::Options,
    helpers::*,
    p_stake_interface::state::{StakeStateV2, StakeStateV2Layout, StakeStateV2View},
    proptest::prelude::*,
    solana_stake_interface::state::StakeStateV2 as LegacyStakeStateV2,
    wincode::ZeroCopy,
};

fn assert_legacy_and_view_agree(bytes: &[u8]) {
    let legacy: LegacyStakeStateV2 = bincode_opts().deserialize(bytes).unwrap();
    let view = StakeStateV2::from_bytes(bytes).unwrap();

    match (legacy, view) {
        (LegacyStakeStateV2::Uninitialized, StakeStateV2View::Uninitialized) => {}
        (LegacyStakeStateV2::RewardsPool, StakeStateV2View::RewardsPool) => {}
        (LegacyStakeStateV2::Initialized(legacy_meta), StakeStateV2View::Initialized(meta)) => {
            assert_meta_compat(meta, &legacy_meta);
        }
        (
            LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, legacy_flags),
            StakeStateV2View::Stake { meta, stake },
        ) => {
            assert_meta_compat(meta, &legacy_meta);
            assert_stake_compat(stake, &legacy_stake);

            // ABI: stake_flags byte must match legacy exactly.
            let layout = StakeStateV2Layout::from_bytes(bytes).unwrap();
            assert_eq!(layout.stake_flags, stake_flags_byte(&legacy_flags));
        }

        (o, v) => panic!("variant mismatch legacy={o:?} new={v:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    // legacy bincode == new layout wincode bytes
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_wincode_roundtrips_legacy_bytes(legacy in arb_legacy_state()) {
        let expected = serialize_legacy(&legacy);
        prop_assert_eq!(expected.len(), LAYOUT_LEN);

        let new_layout = StakeStateV2Layout::from_bytes(&expected[..]).unwrap();
        let mut actual = [0u8; 200];
        wincode::serialize_into(&mut actual.as_mut_slice(), new_layout).unwrap();

        prop_assert_eq!(expected.as_slice(), &actual);
    }

    // both the legacy decoder and zero-copy view interpret trailing bytes the same
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_unpadded_legacy_prefix_is_compatible(legacy in arb_legacy_state(), mut tail in any::<[u8; 200]>()) {
        let prefix = serialize_legacy_unpadded(&legacy);
        tail[..prefix.len()].copy_from_slice(&prefix);
        assert_legacy_and_view_agree(&tail[..]);
    }

    // arbitrary 200-byte blobs with a valid tag must parse identically in legacy bincode and the zero-copy view
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_any_200_bytes_with_valid_tag_legacy_and_new_agree(mut bytes in any::<[u8; 200]>(), tag in 0u32..=3u32) {
        bytes[..4].copy_from_slice(&tag.to_le_bytes());
        assert_legacy_and_view_agree(&bytes[..]);
    }
}
