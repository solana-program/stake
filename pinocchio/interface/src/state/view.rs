//! Read-only zero-copy view into stake account data.

use {
    super::layout::{Meta, Stake, StakeStateV2Layout, StakeStateV2Tag},
    crate::error::StakeStateError,
    core::mem::size_of,
    wincode::ZeroCopy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StakeStateV2View<'a> {
    Uninitialized,
    Initialized(&'a Meta),
    Stake { meta: &'a Meta, stake: &'a Stake },
    RewardsPool,
}

impl<'a> StakeStateV2View<'a> {
    pub(super) fn from_bytes(data: &'a [u8]) -> Result<Self, StakeStateError> {
        if data.len() < size_of::<StakeStateV2Layout>() {
            return Err(StakeStateError::UnexpectedEof);
        }
        let layout = StakeStateV2Layout::from_bytes(data)?;
        let tag = StakeStateV2Tag::from_u32(layout.tag.get())?;

        match tag {
            StakeStateV2Tag::Uninitialized => Ok(Self::Uninitialized),
            StakeStateV2Tag::RewardsPool => Ok(Self::RewardsPool),
            StakeStateV2Tag::Initialized => Ok(Self::Initialized(&layout.meta)),
            StakeStateV2Tag::Stake => Ok(Self::Stake {
                meta: &layout.meta,
                stake: &layout.stake,
            }),
        }
    }
}
