use crate::error::{invalid_tag, slice_as_array, slice_as_array_mut, StakeStateError};
use crate::pod::{PodI64, PodPubkey, PodU32, PodU64};
use core::mem::size_of;
use wincode::{SchemaRead, SchemaWrite};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct AuthorizedBytes {
    pub staker: PodPubkey,
    pub withdrawer: PodPubkey,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct LockupBytes {
    pub unix_timestamp: PodI64,
    pub epoch: PodU64,
    pub custodian: PodPubkey,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct MetaBytes {
    pub rent_exempt_reserve: PodU64,
    pub authorized: AuthorizedBytes,
    pub lockup: LockupBytes,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct DelegationBytes {
    pub voter_pubkey: PodPubkey,
    pub stake: PodU64,
    pub activation_epoch: PodU64,
    pub deactivation_epoch: PodU64,
    pub _reserved: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct StakeBytes {
    pub delegation: DelegationBytes,
    pub credits_observed: PodU64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct StakeStateV2Bytes {
    pub tag: PodU32,
    pub payload: [u8; 196],
}

impl StakeStateV2Bytes {
    pub const SIZE: usize = 200;

    pub const TAG_UNINITIALIZED: u32 = 0;
    pub const TAG_INITIALIZED: u32 = 1;
    pub const TAG_STAKE: u32 = 2;
    pub const TAG_REWARDS_POOL: u32 = 3;

    pub const META_SIZE: usize = size_of::<MetaBytes>();
    pub const STAKE_SIZE: usize = size_of::<StakeBytes>();

    // After meta + stake, payload has: [stake_flags:1][padding:3] = 4 bytes
    pub const FLAGS_OFFSET_IN_PAYLOAD: usize = Self::META_SIZE + Self::STAKE_SIZE;
    pub const TAIL_SIZE: usize = 4;

    #[inline(always)]
    pub fn tag_u32(&self) -> u32 {
        self.tag.get()
    }
}

// Compile-time layout guard
const _: [(); StakeStateV2Bytes::SIZE] = [(); core::mem::size_of::<StakeStateV2Bytes>()];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StakeStateV2View<'a> {
    Uninitialized,
    Initialized(&'a MetaBytes),
    Stake {
        meta: &'a MetaBytes,
        stake: &'a StakeBytes,
        stake_flags: u8,
        padding: &'a [u8; 3],
    },
    RewardsPool,
}

impl<'a> StakeStateV2View<'a> {
    #[inline]
    pub fn from_account_data(data: &'a [u8]) -> Result<Self, StakeStateError> {
        if data.len() != StakeStateV2Bytes::SIZE {
            return Err(StakeStateError::WrongLength {
                expected: StakeStateV2Bytes::SIZE,
                actual: data.len(),
            });
        }

        let raw: &'a StakeStateV2Bytes = wincode::deserialize_ref(data)?;
        let tag = raw.tag_u32();

        match tag {
            StakeStateV2Bytes::TAG_UNINITIALIZED => Ok(Self::Uninitialized),
            StakeStateV2Bytes::TAG_REWARDS_POOL => Ok(Self::RewardsPool),
            StakeStateV2Bytes::TAG_INITIALIZED => {
                let meta_bytes = &raw.payload[..StakeStateV2Bytes::META_SIZE];
                let meta: &'a MetaBytes = wincode::deserialize_ref(meta_bytes)?;
                Ok(Self::Initialized(meta))
            }

            StakeStateV2Bytes::TAG_STAKE => {
                // meta
                let meta_bytes = &raw.payload[..StakeStateV2Bytes::META_SIZE];
                let meta: &'a MetaBytes = wincode::deserialize_ref(meta_bytes)?;

                // stake
                let stake_start = StakeStateV2Bytes::META_SIZE;
                let stake_end = stake_start + StakeStateV2Bytes::STAKE_SIZE;
                let stake_bytes = &raw.payload[stake_start..stake_end];
                let stake: &'a StakeBytes = wincode::deserialize_ref(stake_bytes)?;

                // flags + 3-byte padding tail
                let tail_start = StakeStateV2Bytes::FLAGS_OFFSET_IN_PAYLOAD;
                let tail = &raw.payload[tail_start..];
                if tail.len() != StakeStateV2Bytes::TAIL_SIZE {
                    return Err(StakeStateError::Read(wincode::ReadError::Custom(
                        "stake tail length mismatch",
                    )));
                }
                let stake_flags = tail[0];
                let padding: &'a [u8; 3] = slice_as_array(&tail[1..4])?;

                Ok(Self::Stake {
                    meta,
                    stake,
                    stake_flags,
                    padding,
                })
            }

            other => Err(invalid_tag(other)),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StakeStateV2ViewMut<'a> {
    Uninitialized {
        tag: &'a mut PodU32,
        payload: &'a mut [u8; 196],
    },
    Initialized {
        tag: &'a mut PodU32,
        meta: &'a mut MetaBytes,
    },
    Stake {
        tag: &'a mut PodU32,
        meta: &'a mut MetaBytes,
        stake: &'a mut StakeBytes,
        stake_flags: &'a mut u8,
        padding: &'a mut [u8; 3],
    },
    RewardsPool {
        tag: &'a mut PodU32,
        payload: &'a mut [u8; 196],
    },
}

impl<'a> StakeStateV2ViewMut<'a> {
    #[inline(always)]
    fn split_account(
        data: &'a mut [u8],
    ) -> Result<(&'a mut PodU32, &'a mut [u8; 196]), StakeStateError> {
        if data.len() != StakeStateV2Bytes::SIZE {
            return Err(StakeStateError::WrongLength {
                expected: StakeStateV2Bytes::SIZE,
                actual: data.len(),
            });
        }

        let (tag_region, payload_region) = data.split_at_mut(4);

        let tag: &'a mut PodU32 = wincode::deserialize_mut(tag_region)?;
        let payload: &'a mut [u8; 196] = slice_as_array_mut(payload_region)?;

        Ok((tag, payload))
    }

    #[inline]
    pub fn from_account_data(data: &'a mut [u8]) -> Result<Self, StakeStateError> {
        let (tag, payload) = Self::split_account(data)?;
        let tag_u32 = (*tag).get();

        match tag_u32 {
            StakeStateV2Bytes::TAG_UNINITIALIZED => Ok(Self::Uninitialized { tag, payload }),
            StakeStateV2Bytes::TAG_REWARDS_POOL => Ok(Self::RewardsPool { tag, payload }),

            StakeStateV2Bytes::TAG_INITIALIZED => {
                let meta_region = &mut payload[..StakeStateV2Bytes::META_SIZE];
                let meta: &'a mut MetaBytes = wincode::deserialize_mut(meta_region)?;
                Ok(Self::Initialized { tag, meta })
            }

            StakeStateV2Bytes::TAG_STAKE => {
                // Split payload into meta | stake | tail(4)
                let (meta_region, rest) = payload.split_at_mut(StakeStateV2Bytes::META_SIZE);
                let (stake_region, tail) = rest.split_at_mut(StakeStateV2Bytes::STAKE_SIZE);

                if tail.len() != StakeStateV2Bytes::TAIL_SIZE {
                    return Err(StakeStateError::Read(wincode::ReadError::Custom(
                        "stake tail length mismatch",
                    )));
                }

                let meta: &'a mut MetaBytes = wincode::deserialize_mut(meta_region)?;
                let stake: &'a mut StakeBytes = wincode::deserialize_mut(stake_region)?;

                // tail = [flags(1)][padding(3)]
                let (flags_region, pad_region) = tail.split_at_mut(1);
                let stake_flags: &'a mut u8 = &mut flags_region[0];
                let padding: &'a mut [u8; 3] = slice_as_array_mut(pad_region)?;

                Ok(Self::Stake {
                    tag,
                    meta,
                    stake,
                    stake_flags,
                    padding,
                })
            }

            other => Err(invalid_tag(other)),
        }
    }
}
