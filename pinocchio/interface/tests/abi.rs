mod helpers;

use {
    bincode::Options,
    helpers::*,
    p_stake_interface::state::{StakeStateV2, StakeStateV2Tag},
    proptest::prelude::*,
    solana_stake_interface::state::StakeStateV2 as LegacyStakeStateV2,
};

fn assert_legacy_and_new_layout_agree(bytes: &[u8]) {
    let legacy: LegacyStakeStateV2 = bincode_opts().deserialize(bytes).unwrap();
    let layout = StakeStateV2::from_bytes(bytes).unwrap();

    match legacy {
        LegacyStakeStateV2::Uninitialized => {
            assert_eq!(layout.tag(), StakeStateV2Tag::Uninitialized);
            assert!(layout.meta().is_err());
            assert!(layout.stake().is_err());
        }
        LegacyStakeStateV2::RewardsPool => {
            assert_eq!(layout.tag(), StakeStateV2Tag::RewardsPool);
            assert!(layout.meta().is_err());
            assert!(layout.stake().is_err());
        }
        LegacyStakeStateV2::Initialized(legacy_meta) => {
            assert_eq!(layout.tag(), StakeStateV2Tag::Initialized);
            let meta = layout.meta().unwrap();
            assert_meta_compat(meta, &legacy_meta);
            assert!(layout.stake().is_err());
        }
        LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, legacy_flags) => {
            assert_eq!(layout.tag(), StakeStateV2Tag::Stake);
            let meta = layout.meta().unwrap();
            let stake = layout.stake().unwrap();
            assert_meta_compat(meta, &legacy_meta);
            assert_stake_compat(stake, &legacy_stake);

            // ABI: stake_flags byte must match legacy exactly.
            assert_eq!(bytes[FLAGS_OFF], stake_flags_byte(&legacy_flags));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    // legacy bincode == new layout wincode bytes
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_wincode_roundtrips_legacy_bytes(legacy in arb_legacy_state()) {
        let expected = serialize_legacy(&legacy);
        prop_assert_eq!(expected.len(), STATE_LEN);

        let new_layout = StakeStateV2::from_bytes(&expected[..]).unwrap();
        let mut actual = [0u8; 200];
        wincode::serialize_into(actual.as_mut_slice(), new_layout).unwrap();

        prop_assert_eq!(expected.as_slice(), &actual);
    }

    // both the legacy decoder and zero-copy layout interpret trailing bytes the same
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_unpadded_legacy_prefix_is_compatible(legacy in arb_legacy_state(), mut tail in any::<[u8; 200]>()) {
        let prefix = serialize_legacy_unpadded(&legacy);
        tail[..prefix.len()].copy_from_slice(&prefix);
        assert_legacy_and_new_layout_agree(&tail[..]);
    }

    // arbitrary 200-byte blobs with a valid tag must parse identically in legacy bincode and the zero-copy layout
    #[test]
    #[cfg_attr(miri, ignore)]
    fn prop_any_200_bytes_with_valid_tag_legacy_and_new_agree(mut bytes in any::<[u8; 200]>(), tag in 0u32..=3u32) {
        bytes[..4].copy_from_slice(&tag.to_le_bytes());
        assert_legacy_and_new_layout_agree(&bytes[..]);
    }
}
