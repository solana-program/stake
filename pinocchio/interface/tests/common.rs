#![allow(dead_code)]

use {
    bincode::Options,
    core::mem::size_of,
    p_stake_interface::state::{PodAddress, StakeStateV2Layout, StakeStateV2Tag},
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_flags::StakeFlags as OldStakeFlags,
        state::{
            Authorized as OldAuthorized, Delegation as OldDelegation, Lockup as OldLockup,
            Meta as OldMeta, Stake as OldStake, StakeStateV2 as OldStakeStateV2,
        },
    },
};

pub fn bincode_opts() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

pub fn serialize_old(state: &OldStakeStateV2) -> Vec<u8> {
    let mut data = bincode_opts().serialize(state).unwrap();
    data.resize(size_of::<StakeStateV2Layout>(), 0);
    data
}

pub fn deserialize_old(data: &[u8]) -> OldStakeStateV2 {
    bincode_opts().deserialize::<OldStakeStateV2>(data).unwrap()
}

pub fn empty_state_bytes(tag: StakeStateV2Tag) -> [u8; 200] {
    let mut data = [0u8; size_of::<StakeStateV2Layout>()];
    let mut slice = &mut data[..StakeStateV2Tag::TAG_LEN];
    wincode::serialize_into(&mut slice, &tag).unwrap();
    data
}

pub fn assert_200_bytes(bytes: &[u8]) {
    assert_eq!(bytes.len(), size_of::<StakeStateV2Layout>());
    assert_eq!(bytes.len(), 200);
}

pub fn assert_tag(bytes: &[u8], expected: StakeStateV2Tag) {
    let tag = StakeStateV2Tag::from_bytes(bytes).unwrap();
    assert_eq!(tag, expected);
}

/// Creates a Pubkey from bytes
pub fn pk(bytes: [u8; 32]) -> Pubkey {
    Pubkey::new_from_array(bytes)
}

/// Creates a Pubkey with a repeated byte
pub fn pk_u8(byte: u8) -> Pubkey {
    pk([byte; 32])
}

/// Converts PodAddress to Pubkey for direct comparison
pub fn to_pubkey(pod: &PodAddress) -> Pubkey {
    Pubkey::new_from_array(*pod.as_bytes())
}

pub fn legacy_uninitialized() -> OldStakeStateV2 {
    OldStakeStateV2::Uninitialized
}

pub fn legacy_rewards_pool() -> OldStakeStateV2 {
    OldStakeStateV2::RewardsPool
}

pub fn legacy_initialized() -> OldStakeStateV2 {
    OldStakeStateV2::Initialized(OldMeta {
        rent_exempt_reserve: u64::MAX,
        authorized: OldAuthorized {
            staker: pk_u8(0x11),
            withdrawer: pk_u8(0x22),
        },
        lockup: OldLockup {
            unix_timestamp: i64::MIN + 1,
            epoch: u64::MAX,
            custodian: pk_u8(0x33),
        },
    })
}

#[allow(deprecated)]
pub fn legacy_stake() -> OldStakeStateV2 {
    let old_meta = OldMeta {
        rent_exempt_reserve: 1,
        authorized: OldAuthorized {
            staker: pk_u8(0x44),
            withdrawer: pk_u8(0x55),
        },
        lockup: OldLockup {
            unix_timestamp: -1,
            epoch: 1,
            custodian: pk_u8(0x66),
        },
    };

    let old_delegation = OldDelegation {
        voter_pubkey: pk_u8(0x77),
        stake: u64::MAX,
        activation_epoch: 0,
        deactivation_epoch: u64::MAX,
        #[allow(deprecated)]
        warmup_cooldown_rate: f64::from_le_bytes([0xAA; 8]),
    };

    let old_stake = OldStake {
        delegation: old_delegation,
        credits_observed: u64::MAX - 1,
    };

    let mut old_flags = OldStakeFlags::empty();
    #[allow(deprecated)]
    old_flags.set(OldStakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);

    OldStakeStateV2::Stake(old_meta, old_stake, old_flags)
}
