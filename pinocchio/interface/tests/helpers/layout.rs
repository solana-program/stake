#![allow(deprecated)]
use {
    super::legacy::bincode_opts,
    bincode::Options,
    core::mem::size_of,
    p_stake_interface::{
        pod::{Address, PodI64, PodU64},
        state::{Authorized, Delegation, Lockup, Meta, Stake, StakeStateV2, StakeStateV2Tag},
    },
    solana_stake_interface::{
        stake_flags::StakeFlags as LegacyStakeFlags,
        state::{Meta as LegacyMeta, Stake as LegacyStake},
    },
};

pub const TAG_LEN: usize = StakeStateV2Tag::TAG_LEN;
pub const STATE_LEN: usize = size_of::<StakeStateV2>();

pub const META_OFF: usize = TAG_LEN;
pub const STAKE_OFF: usize = TAG_LEN + size_of::<Meta>();
pub const FLAGS_OFF: usize = TAG_LEN + size_of::<Meta>() + size_of::<Stake>();
pub const PADDING_OFF: usize = FLAGS_OFF + 1;

pub fn write_tag(bytes: &mut [u8], tag: StakeStateV2Tag) {
    bytes[..TAG_LEN].copy_from_slice(&(tag as u32).to_le_bytes());
}

pub fn empty_state_bytes(tag: StakeStateV2Tag) -> [u8; 200] {
    let mut data = [0u8; size_of::<StakeStateV2>()];
    data[..TAG_LEN].copy_from_slice(&(tag as u32).to_le_bytes());
    data
}

pub fn stake_flags_byte(legacy_flags: &LegacyStakeFlags) -> u8 {
    let bs = bincode_opts().serialize(legacy_flags).unwrap();
    assert_eq!(bs.len(), 1);
    bs[0]
}

pub fn warmup_reserved_bytes_from_legacy_rate(legacy_rate: f64) -> [u8; 8] {
    legacy_rate.to_bits().to_le_bytes()
}

pub fn meta_from_legacy(legacy: &LegacyMeta) -> Meta {
    Meta {
        rent_exempt_reserve: PodU64::from_primitive(legacy.rent_exempt_reserve),
        authorized: Authorized {
            staker: Address::new_from_array(legacy.authorized.staker.to_bytes()),
            withdrawer: Address::new_from_array(legacy.authorized.withdrawer.to_bytes()),
        },
        lockup: Lockup {
            unix_timestamp: PodI64::from_primitive(legacy.lockup.unix_timestamp),
            epoch: PodU64::from_primitive(legacy.lockup.epoch),
            custodian: Address::new_from_array(legacy.lockup.custodian.to_bytes()),
        },
    }
}

pub fn assert_meta_compat(new: &Meta, legacy: &LegacyMeta) {
    assert_eq!(new.rent_exempt_reserve.get(), legacy.rent_exempt_reserve);
    assert_eq!(
        new.authorized.staker.to_bytes(),
        legacy.authorized.staker.to_bytes()
    );
    assert_eq!(
        new.authorized.withdrawer.to_bytes(),
        legacy.authorized.withdrawer.to_bytes()
    );
    assert_eq!(
        new.lockup.unix_timestamp.get(),
        legacy.lockup.unix_timestamp
    );
    assert_eq!(new.lockup.epoch.get(), legacy.lockup.epoch);
    assert_eq!(
        new.lockup.custodian.to_bytes(),
        legacy.lockup.custodian.to_bytes()
    );
}

pub fn assert_stake_compat(new: &Stake, legacy: &LegacyStake) {
    assert_eq!(
        new.delegation.voter_pubkey.to_bytes(),
        legacy.delegation.voter_pubkey.to_bytes()
    );
    assert_eq!(new.delegation.stake.get(), legacy.delegation.stake);
    assert_eq!(
        new.delegation.activation_epoch.get(),
        legacy.delegation.activation_epoch
    );
    assert_eq!(
        new.delegation.deactivation_epoch.get(),
        legacy.delegation.deactivation_epoch
    );
    let expected_reserved =
        warmup_reserved_bytes_from_legacy_rate(legacy.delegation.warmup_cooldown_rate);
    assert_eq!(new.delegation._reserved, expected_reserved);
    assert_eq!(new.credits_observed.get(), legacy.credits_observed);
}
