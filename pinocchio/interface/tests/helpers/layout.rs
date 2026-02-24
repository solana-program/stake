#![allow(deprecated)]
use {
    super::legacy::bincode_opts,
    bincode::Options,
    core::mem::size_of,
    p_stake_interface::state::{Meta, Stake, StakeStateV2, StakeStateV2Tag},
    solana_stake_interface::{
        stake_flags::StakeFlags as LegacyStakeFlags,
        state::{Meta as LegacyMeta, Stake as LegacyStake},
    },
    spl_pod::primitives::PodU64,
};

pub const TAG_LEN: usize = StakeStateV2Tag::TAG_LEN;
pub const STATE_LEN: usize = size_of::<StakeStateV2>();

pub const META_OFFSET: usize = TAG_LEN;
pub const STAKE_OFFSET: usize = TAG_LEN + size_of::<Meta>();
pub const PADDING_OFFSET: usize = TAG_LEN + size_of::<Meta>() + size_of::<Stake>();

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

pub fn assert_meta_compat(new: &Meta, legacy: &LegacyMeta) {
    assert_eq!(
        u64::from(new.rent_exempt_reserve),
        legacy.rent_exempt_reserve
    );
    assert_eq!(
        new.authorized.staker.to_bytes(),
        legacy.authorized.staker.to_bytes()
    );
    assert_eq!(
        new.authorized.withdrawer.to_bytes(),
        legacy.authorized.withdrawer.to_bytes()
    );
    assert_eq!(
        i64::from(new.lockup.unix_timestamp),
        legacy.lockup.unix_timestamp
    );
    assert_eq!(u64::from(new.lockup.epoch), legacy.lockup.epoch);
    assert_eq!(
        new.lockup.custodian.to_bytes(),
        legacy.lockup.custodian.to_bytes()
    );
}

pub fn overwrite_tail(bytes: &mut [u8], tail: [u8; 4]) -> [u8; 4] {
    bytes[PADDING_OFFSET..STATE_LEN].copy_from_slice(&tail);
    tail
}

pub fn assert_tail(layout_bytes: &[u8], tail: [u8; 4]) {
    assert_eq!(&layout_bytes[PADDING_OFFSET..STATE_LEN], &tail);
}

pub fn assert_tail_zeroed(layout_bytes: &[u8]) {
    assert_eq!(&layout_bytes[PADDING_OFFSET..STATE_LEN], &[0u8; 4]);
}

/// Verifies that the deserialized layout is a true zero-copy borrow into the original byte slice.
pub fn assert_borrows_at<T>(borrow: &T, bytes: &[u8], offset: usize) {
    let ptr = borrow as *const T;
    let expected = unsafe { bytes.as_ptr().add(offset) };
    assert_eq!(ptr as *const u8, expected);
}

/// Verifies that the deserialized mutable view is a true zero-copy borrow into the original
/// byte slice at the given offset.
pub fn assert_mut_borrows_at<T>(borrow: &mut T, base_ptr: *mut u8, offset: usize) {
    let ptr = borrow as *mut T as *mut u8;
    let expected = unsafe { base_ptr.add(offset) };
    assert_eq!(ptr, expected);
}

pub fn assert_stake_compat(new: &Stake, legacy: &LegacyStake) {
    assert_eq!(
        new.delegation.voter_pubkey.to_bytes(),
        legacy.delegation.voter_pubkey.to_bytes()
    );
    assert_eq!(u64::from(new.delegation.stake), legacy.delegation.stake);
    assert_eq!(
        u64::from(new.delegation.activation_epoch),
        legacy.delegation.activation_epoch
    );
    assert_eq!(
        u64::from(new.delegation.deactivation_epoch),
        legacy.delegation.deactivation_epoch
    );
    let expected_reserved =
        warmup_reserved_bytes_from_legacy_rate(legacy.delegation.warmup_cooldown_rate);
    assert_eq!(new.delegation._reserved, expected_reserved);
    assert_eq!(u64::from(new.credits_observed), legacy.credits_observed);
}
