//! Zero-copy stake state types.

use {
    crate::{
        error::StakeStateError,
        pod::{Address, PodI64, PodU32, PodU64},
    },
    core::mem::size_of,
    wincode::{SchemaRead, SchemaWrite, ZeroCopy},
};

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct Authorized {
    pub staker: Address,
    pub withdrawer: Address,
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct Lockup {
    /// `UnixTimestamp` at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian.
    pub unix_timestamp: PodI64,
    /// Epoch height at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian.
    pub epoch: PodU64,
    /// Custodian signature on a transaction exempts the operation from
    ///  lockup constraints.
    pub custodian: Address,
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, SchemaWrite, SchemaRead, Default)]
#[wincode(assert_zero_copy)]
pub struct Meta {
    pub rent_exempt_reserve: PodU64,
    pub authorized: Authorized,
    pub lockup: Lockup,
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct Delegation {
    /// To whom the stake is delegated.
    pub voter_pubkey: Address,
    /// Activated stake amount, set at delegate() time.
    pub stake: PodU64,
    /// Epoch at which this stake was activated, `u64::MAX` if is a bootstrap stake.
    pub activation_epoch: PodU64,
    /// Epoch the stake was deactivated, `u64::MAX` if not deactivated.
    pub deactivation_epoch: PodU64,
    /// Reserved bytes (formerly warmup/cooldown rate).
    /// Deprecated in the runtime but preserved for ABI compatibility.
    pub _reserved: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, SchemaWrite, SchemaRead, Default)]
#[wincode(assert_zero_copy)]
pub struct Stake {
    pub delegation: Delegation,
    /// Credits observed is credits from vote account state when delegated or redeemed.
    pub credits_observed: PodU64,
}

/// Discriminant tag for stake account state (first 4 bytes).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StakeStateV2Tag {
    Uninitialized = 0,
    Initialized = 1,
    Stake = 2,
    RewardsPool = 3,
}

impl StakeStateV2Tag {
    pub const TAG_LEN: usize = size_of::<PodU32>();

    #[inline]
    pub(crate) fn from_u32(v: u32) -> Result<Self, StakeStateError> {
        Self::assert_valid_tag(v)?;
        Ok(unsafe { core::mem::transmute::<u32, StakeStateV2Tag>(v) })
    }

    #[inline]
    pub(crate) unsafe fn from_u32_unchecked(v: u32) -> Self {
        debug_assert!(v <= Self::RewardsPool as u32);
        core::mem::transmute::<u32, StakeStateV2Tag>(v)
    }

    #[inline]
    pub(crate) fn assert_valid_tag(v: u32) -> Result<(), StakeStateError> {
        match v {
            0..=3 => Ok(()),
            other => Err(StakeStateError::InvalidTag(other)),
        }
    }

    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StakeStateError> {
        if bytes.len() < Self::TAG_LEN {
            return Err(StakeStateError::UnexpectedEof);
        }
        let raw = u32::from_le_bytes(bytes[..Self::TAG_LEN].try_into().unwrap());
        Self::from_u32(raw)
    }
}

/// 200-byte stake account layout:
///
/// ```text
/// ┌────────┬──────┬────────────┐
/// │ Offset │ Size │ Field      │
/// ├────────┼──────┼────────────┤
/// │   0    │  4   │ Tag        │
/// │   4    │ 120  │ Meta       │
/// │  124   │  72  │ Stake      │
/// │  196   │  4   │ Padding    │
/// └────────┴──────┴────────────┘
/// ```
///
/// All fields are alignment-1 for safe zero-copy from unaligned byte slices.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct StakeStateV2 {
    tag: PodU32,
    meta: Meta,
    stake: Stake,
    stake_flags: u8,
    padding: [u8; 3],
}

// compile-time size check
const _: () = assert!(size_of::<StakeStateV2>() == 200);

impl StakeStateV2 {
    /// The fixed size of a stake account in bytes.
    pub const LEN: usize = size_of::<StakeStateV2>();

    /// Parse stake account data into a read-only reference.
    pub fn from_bytes(data: &[u8]) -> Result<&Self, StakeStateError> {
        let state = <Self as ZeroCopy>::from_bytes(data).map_err(|_| StakeStateError::Decode)?;
        StakeStateV2Tag::assert_valid_tag(state.tag.get())?;
        Ok(state)
    }

    /// Parse stake account data into a mutable reference.
    pub fn from_bytes_mut(data: &mut [u8]) -> Result<&mut Self, StakeStateError> {
        let state =
            <Self as ZeroCopy>::from_bytes_mut(data).map_err(|_| StakeStateError::Decode)?;
        StakeStateV2Tag::assert_valid_tag(state.tag.get())?;
        Ok(state)
    }

    /// Returns the state tag (infallible since validated at construction).
    #[inline]
    pub fn tag(&self) -> StakeStateV2Tag {
        // SAFETY: tag validated at construction
        unsafe { StakeStateV2Tag::from_u32_unchecked(self.tag.get()) }
    }

    /// Returns a reference to `Meta` if in the `Initialized` or `Stake` state.
    #[inline]
    pub fn meta(&self) -> Option<&Meta> {
        match self.tag() {
            StakeStateV2Tag::Initialized | StakeStateV2Tag::Stake => Some(&self.meta),
            _ => None,
        }
    }

    /// Returns a reference to `Stake` if in the `Stake` state.
    #[inline]
    pub fn stake(&self) -> Option<&Stake> {
        match self.tag() {
            StakeStateV2Tag::Stake => Some(&self.stake),
            _ => None,
        }
    }

    /// Returns a mutable reference to `Meta` if in the `Initialized` or `Stake` state.
    #[inline]
    pub fn meta_mut(&mut self) -> Result<&mut Meta, StakeStateError> {
        match self.tag() {
            StakeStateV2Tag::Initialized | StakeStateV2Tag::Stake => Ok(&mut self.meta),
            tag => Err(StakeStateError::InvalidStateAccess(tag)),
        }
    }

    /// Returns a mutable reference to `Stake` if in the `Stake` state.
    #[inline]
    pub fn stake_mut(&mut self) -> Result<&mut Stake, StakeStateError> {
        match self.tag() {
            StakeStateV2Tag::Stake => Ok(&mut self.stake),
            tag => Err(StakeStateError::InvalidStateAccess(tag)),
        }
    }

    /// Transition from `Uninitialized` to `Initialized`.
    /// Clears the stake and tail regions.
    pub fn initialize(&mut self, meta: Meta) -> Result<(), StakeStateError> {
        let from = self.tag();
        if from != StakeStateV2Tag::Uninitialized {
            return Err(StakeStateError::InvalidTransition {
                from,
                to: StakeStateV2Tag::Initialized,
            });
        }

        self.stake = Stake::default();
        self.stake_flags = 0;
        self.padding.fill(0);
        self.meta = meta;
        self.tag.set(StakeStateV2Tag::Initialized as u32);

        Ok(())
    }

    /// Transition to `Stake` state from `Initialized` or `Stake`.
    /// - From `Initialized`: clears tail region to zero.
    /// - From `Stake`: preserves existing tail region.
    pub fn delegate(&mut self, meta: Meta, stake: Stake) -> Result<(), StakeStateError> {
        let from = self.tag();
        if !matches!(from, StakeStateV2Tag::Initialized | StakeStateV2Tag::Stake) {
            return Err(StakeStateError::InvalidTransition {
                from,
                to: StakeStateV2Tag::Stake,
            });
        }

        if from == StakeStateV2Tag::Initialized {
            self.stake_flags = 0;
            self.padding.fill(0);
        }

        self.meta = meta;
        self.stake = stake;
        self.tag.set(StakeStateV2Tag::Stake as u32);

        Ok(())
    }
}
