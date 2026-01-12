//! Mutable handle and view for stake account data.

use {
    super::{
        layout::{Meta, Stake, StakeStateV2Layout, StakeStateV2Tag},
        pod::PodU32,
        view::StakeStateV2View,
    },
    crate::error::StakeStateError,
    core::mem::size_of,
    wincode::ZeroCopy,
};

/// Mutable handle for stake account state transitions.
#[derive(Debug)]
pub struct StakeStateV2Writer<'a> {
    data: &'a mut [u8],
}

impl<'a> StakeStateV2Writer<'a> {
    pub(super) fn from_bytes_mut(data: &'a mut [u8]) -> Result<Self, StakeStateError> {
        if data.len() < size_of::<StakeStateV2Layout>() {
            return Err(StakeStateError::UnexpectedEof);
        }

        let layout = StakeStateV2Layout::from_bytes_mut(data)?;
        StakeStateV2Tag::from_u32(layout.tag.get())?;

        Ok(Self { data })
    }

    pub fn view(&self) -> Result<StakeStateV2View<'_>, StakeStateError> {
        StakeStateV2View::from_bytes(self.data)
    }

    pub fn view_mut(&mut self) -> Result<StakeStateV2ViewMut<'_>, StakeStateError> {
        StakeStateV2ViewMut::from_bytes_mut(self.data)
    }

    /// Transition to `Initialized` state. Only valid from `Uninitialized` state.
    pub fn into_initialized(self, meta: Meta) -> Result<Self, StakeStateError> {
        self.check_transition(StakeStateV2Tag::Initialized)?;

        let layout = StakeStateV2Layout::from_bytes_mut(self.data)?;

        // Clear stake and tail regions
        layout.stake = Stake::default();
        layout.stake_flags = 0;
        layout.padding.fill(0);

        // Set meta and tag
        layout.meta = meta;
        layout.tag.set(StakeStateV2Tag::Initialized as u32);

        Ok(Self { data: self.data })
    }

    /// Transition to Stake state. Only valid from `Initialized` or `Stake` state.
    /// When transitioning from `Initialized`, clears `stake_flags` to 0.
    /// When staying in `Stake`, preserves existing `stake_flags`.
    pub fn into_stake(self, meta: Meta, stake: Stake) -> Result<Self, StakeStateError> {
        self.check_transition(StakeStateV2Tag::Stake)?;
        let from_initialized = self.tag()? == StakeStateV2Tag::Initialized;

        let layout = StakeStateV2Layout::from_bytes_mut(self.data)?;

        // Only clear tail region on Initialized -> Stake
        if from_initialized {
            layout.stake_flags = 0;
            layout.padding.fill(0);
        }

        layout.meta = meta;
        layout.stake = stake;
        layout.tag.set(StakeStateV2Tag::Stake as u32);

        Ok(Self { data: self.data })
    }

    fn tag(&self) -> Result<StakeStateV2Tag, StakeStateError> {
        StakeStateV2Tag::from_bytes(&self.data[..size_of::<PodU32>()])
    }

    fn check_transition(&self, to: StakeStateV2Tag) -> Result<(), StakeStateError> {
        let from = self.tag()?;
        match (from, to) {
            (StakeStateV2Tag::Uninitialized, StakeStateV2Tag::Initialized)
            | (StakeStateV2Tag::Initialized, StakeStateV2Tag::Stake)
            | (StakeStateV2Tag::Stake, StakeStateV2Tag::Stake) => Ok(()),
            _ => Err(StakeStateError::InvalidTransition { from, to }),
        }
    }
}

/// Mutable view into stake account data for in-place field mutations.
#[derive(Debug, PartialEq, Eq)]
pub enum StakeStateV2ViewMut<'a> {
    Uninitialized,
    Initialized(&'a mut Meta),
    Stake {
        meta: &'a mut Meta,
        stake: &'a mut Stake,
    },
    RewardsPool,
}

impl<'a> StakeStateV2ViewMut<'a> {
    pub(super) fn from_bytes_mut(data: &'a mut [u8]) -> Result<Self, StakeStateError> {
        if data.len() < size_of::<StakeStateV2Layout>() {
            return Err(StakeStateError::UnexpectedEof);
        }
        let layout = StakeStateV2Layout::from_bytes_mut(data)?;
        let tag = StakeStateV2Tag::from_u32(layout.tag.get())?;

        match tag {
            StakeStateV2Tag::Uninitialized => Ok(Self::Uninitialized),
            StakeStateV2Tag::RewardsPool => Ok(Self::RewardsPool),
            StakeStateV2Tag::Initialized => Ok(Self::Initialized(&mut layout.meta)),
            StakeStateV2Tag::Stake => Ok(Self::Stake {
                meta: &mut layout.meta,
                stake: &mut layout.stake,
            }),
        }
    }
}
