#![allow(deprecated)]
mod helpers;

use {
    helpers::*,
    p_stake_interface::{error::StakeStateError, state::StakeStateV2Tag},
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

fn serialize_tag(tag: StakeStateV2Tag) -> [u8; 4] {
    let mut buf = [0u8; 4];
    wincode::serialize_into(&mut buf.as_mut_slice(), &tag).unwrap();
    buf
}

fn legacy_state_for_tag(tag: StakeStateV2Tag) -> LegacyStakeStateV2 {
    match tag {
        StakeStateV2Tag::Uninitialized => LegacyStakeStateV2::Uninitialized,
        StakeStateV2Tag::RewardsPool => LegacyStakeStateV2::RewardsPool,
        StakeStateV2Tag::Initialized => LegacyStakeStateV2::Initialized(LegacyMeta {
            rent_exempt_reserve: u64::MAX,
            authorized: LegacyAuthorized {
                staker: Pubkey::new_from_array([17; 32]),
                withdrawer: Pubkey::new_from_array([34; 32]),
            },
            lockup: LegacyLockup {
                unix_timestamp: 123,
                epoch: u64::MAX,
                custodian: Pubkey::new_from_array([51; 32]),
            },
        }),
        StakeStateV2Tag::Stake => {
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

            let legacy_delegation = LegacyDelegation {
                voter_pubkey: Pubkey::new_from_array([119; 32]),
                stake: u64::MAX,
                activation_epoch: 0,
                deactivation_epoch: u64::MAX,
                warmup_cooldown_rate: f64::from_le_bytes([170; 8]),
            };

            let legacy_stake = LegacyStake {
                delegation: legacy_delegation,
                credits_observed: u64::MAX - 1,
            };

            let mut legacy_flags = LegacyStakeFlags::empty();
            legacy_flags
                .set(LegacyStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);

            LegacyStakeStateV2::Stake(legacy_meta, legacy_stake, legacy_flags)
        }
    }
}

#[test]
fn tag_len_is_4_bytes() {
    assert_eq!(StakeStateV2Tag::TAG_LEN, 4);
}

#[test_case(StakeStateV2Tag::Uninitialized, 0)]
#[test_case(StakeStateV2Tag::Initialized, 1)]
#[test_case(StakeStateV2Tag::Stake, 2)]
#[test_case(StakeStateV2Tag::RewardsPool, 3)]
fn tag_serializes_to_expected_u32_le_discriminant(tag: StakeStateV2Tag, expected: u32) {
    let buf = serialize_tag(tag);
    assert_eq!(buf, expected.to_le_bytes());
}

#[test]
fn from_u32_errors_on_invalid_values() {
    let err = StakeStateV2Tag::from_u32(4).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(4)));

    let err = StakeStateV2Tag::from_u32(u32::MAX).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(u32::MAX)));
}

#[test_case(0u32, StakeStateV2Tag::Uninitialized)]
#[test_case(1u32, StakeStateV2Tag::Initialized)]
#[test_case(2u32, StakeStateV2Tag::Stake)]
#[test_case(3u32, StakeStateV2Tag::RewardsPool)]
fn from_bytes_decodes_all_valid_tags(expected: u32, tag: StakeStateV2Tag) {
    let bytes = expected.to_le_bytes();
    let decoded = StakeStateV2Tag::from_bytes(&bytes[..]).unwrap();
    assert_eq!(decoded, tag);
}

#[test]
fn from_bytes_ignores_trailing_data() {
    // from_bytes() should only look at the first 4 bytes and ignore any trailing data.
    let mut bytes = [0u8; 9];
    bytes[..4].copy_from_slice(&2u32.to_le_bytes());
    bytes[4..].copy_from_slice(&[171; 5]);

    let decoded = StakeStateV2Tag::from_bytes(&bytes[..]).unwrap();
    assert_eq!(decoded, StakeStateV2Tag::Stake);
}

#[test]
fn from_bytes_rejects_invalid_discriminant() {
    let bytes = 999u32.to_le_bytes();
    let err = StakeStateV2Tag::from_bytes(&bytes[..]).unwrap_err();
    assert!(matches!(err, StakeStateError::InvalidTag(999)));
}

#[test]
fn from_bytes_rejects_short_buffer() {
    let bytes = [0u8; 3];
    let err = StakeStateV2Tag::from_bytes(&bytes[..]).unwrap_err();
    assert!(matches!(err, StakeStateError::UnexpectedEof));
}

#[test_case(StakeStateV2Tag::Uninitialized)]
#[test_case(StakeStateV2Tag::Initialized)]
#[test_case(StakeStateV2Tag::Stake)]
#[test_case(StakeStateV2Tag::RewardsPool)]
fn tag_encoding_matches_legacy_bincode_discriminant(tag: StakeStateV2Tag) {
    // This enforces the compatibility requirement: legacy StakeStateV2 (bincode with our opts)
    // must have the same 4-byte LE u32 discriminant as the wincode-encoded StakeStateV2Tag.
    let legacy_state = legacy_state_for_tag(tag);
    let legacy_bytes = serialize_legacy(&legacy_state);
    assert!(legacy_bytes.len() >= StakeStateV2Tag::TAG_LEN);

    let legacy_tag = &legacy_bytes[..StakeStateV2Tag::TAG_LEN];
    let wincode_tag = serialize_tag(tag);

    assert_eq!(legacy_tag, &wincode_tag[..]);
}
