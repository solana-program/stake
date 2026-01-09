use {
    super::pod::{PodAddress, PodI64, PodU32, PodU64},
    crate::error::StakeStateError,
    core::mem::size_of,
    wincode::{Deserialize, ReadError, SchemaRead, SchemaWrite},
};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct Authorized {
    pub staker: PodAddress,
    pub withdrawer: PodAddress,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct Lockup {
    /// `UnixTimestamp` at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub unix_timestamp: PodI64,
    /// epoch height at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub epoch: PodU64,
    /// custodian signature on a transaction exempts the operation from
    ///  lockup constraints
    pub custodian: PodAddress,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead, Default)]
#[wincode(assert_zero_copy)]
pub struct Meta {
    pub rent_exempt_reserve: PodU64,
    pub authorized: Authorized,
    pub lockup: Lockup,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct Delegation {
    /// to whom the stake is delegated
    pub voter_pubkey: PodAddress,
    /// activated stake amount, set at delegate() time
    pub stake: PodU64,
    /// epoch at which this stake was activated, `std::u64::MAX` if is a bootstrap stake
    pub activation_epoch: PodU64,
    /// epoch the stake was deactivated, `std::u64::MAX` if not deactivated
    pub deactivation_epoch: PodU64,
    /// Legacy bytes from legacy warmup/cooldown rate encoding (deprecated).
    pub _reserved: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead, Default)]
#[wincode(assert_zero_copy)]
pub struct Stake {
    pub delegation: Delegation,
    /// credits observed is credits from vote account state when delegated or redeemed
    pub credits_observed: PodU64,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u32")]
pub enum StakeStateV2Tag {
    #[wincode(tag = 0)]
    Uninitialized = 0,
    #[wincode(tag = 1)]
    Initialized = 1,
    #[wincode(tag = 2)]
    Stake = 2,
    #[wincode(tag = 3)]
    RewardsPool = 3,
}

impl StakeStateV2Tag {
    pub const TAG_LEN: usize = size_of::<PodU32>();

    pub fn from_u32(v: u32) -> Result<Self, StakeStateError> {
        match v {
            0 => Ok(Self::Uninitialized),
            1 => Ok(Self::Initialized),
            2 => Ok(Self::Stake),
            3 => Ok(Self::RewardsPool),
            other => Err(StakeStateError::InvalidTag(other)),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StakeStateError> {
        if bytes.len() < Self::TAG_LEN {
            return Err(StakeStateError::UnexpectedEof);
        }

        let tag_bytes = &bytes[..Self::TAG_LEN];
        StakeStateV2Tag::deserialize(tag_bytes).map_err(|e| match e {
            ReadError::InvalidTagEncoding(tag) => StakeStateError::InvalidTag(tag as u32),
            other => StakeStateError::Read(other),
        })
    }
}

/// Raw 200-byte stake account data with this structure:
///
/// ```text
/// ┌────────┬──────┬────────────┐
/// │ Offset │ Size │ Field      │
/// ├────────┼──────┼────────────┤
/// │   0    │  4   │ Tag        │
/// │   4    │ 120  │ Meta       │
/// │  124   │  72  │ Stake      │
/// │  196   │  1   │ StakeFlags │
/// │  197   │  3   │ Padding    │
/// └────────┴──────┴────────────┘
/// ```
///
/// All structs have alignment 1 for safe zero-copy from unaligned `&[u8]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct StakeStateV2Layout {
    pub tag: PodU32,
    pub meta: Meta,
    pub stake: Stake,
    pub stake_flags: u8,
    pub padding: [u8; 3],
}

// ======= Compile-time size guards =======
const _: () = assert!(size_of::<StakeStateV2Layout>() == 200);
const _: () = assert!(size_of::<StakeStateV2Tag>() == 4);
const _: () = assert!(size_of::<PodU32>() == 4);
const _: () = assert!(size_of::<Meta>() == 120);
const _: () = assert!(size_of::<Stake>() == 72);
const _: () = assert!(size_of::<Authorized>() == 64);
const _: () = assert!(size_of::<Lockup>() == 48);
const _: () = assert!(size_of::<Delegation>() == 64);

// ======= Compile-time alignment guards =======
const _: () = assert!(align_of::<StakeStateV2Layout>() == 1);
const _: () = assert!(align_of::<Meta>() == 1);
const _: () = assert!(align_of::<Stake>() == 1);
const _: () = assert!(align_of::<Delegation>() == 1);
