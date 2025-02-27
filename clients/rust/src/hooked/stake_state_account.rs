use {
    crate::generated::types::{
        Authorized, Delegation, Lockup, Meta, Stake, StakeFlags, StakeStateV2,
    },
    borsh::{BorshDeserialize, BorshSerialize},
    std::io::{Error, ErrorKind},
};

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StakeStateAccount {
    state: StakeStateV2,
}

impl StakeStateAccount {
    #[inline(always)]
    pub fn from_bytes(data: &[u8]) -> Result<Self, std::io::Error> {
        let mut data = data;
        Self::deserialize(&mut data)
    }

    pub const fn size_of() -> usize {
        200
    }

    pub fn stake(&self) -> Option<Stake> {
        match &self.state {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => Some(stake.clone()),
            StakeStateV2::Uninitialized
            | StakeStateV2::Initialized(_)
            | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn stake_ref(&self) -> Option<&Stake> {
        match &self.state {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => Some(stake),
            StakeStateV2::Uninitialized
            | StakeStateV2::Initialized(_)
            | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn stake_flags(&self) -> Option<StakeFlags> {
        match &self.state {
            StakeStateV2::Stake(_meta, _stake, stake_flags) => Some(stake_flags.clone()),
            StakeStateV2::Uninitialized
            | StakeStateV2::Initialized(_)
            | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn stake_flags_ref(&self) -> Option<&StakeFlags> {
        match &self.state {
            StakeStateV2::Stake(_meta, _stake, stake_flags) => Some(stake_flags),
            StakeStateV2::Uninitialized
            | StakeStateV2::Initialized(_)
            | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn delegation(&self) -> Option<Delegation> {
        match &self.state {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => Some(stake.delegation.clone()),
            StakeStateV2::Uninitialized
            | StakeStateV2::Initialized(_)
            | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn delegation_ref(&self) -> Option<&Delegation> {
        match &self.state {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => Some(&stake.delegation),
            StakeStateV2::Uninitialized
            | StakeStateV2::Initialized(_)
            | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn authorized(&self) -> Option<Authorized> {
        match &self.state {
            StakeStateV2::Stake(meta, _stake, _stake_flags) => Some(meta.authorized.clone()),
            StakeStateV2::Initialized(meta) => Some(meta.authorized.clone()),
            StakeStateV2::Uninitialized | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn lockup(&self) -> Option<Lockup> {
        self.meta().map(|meta| meta.lockup)
    }

    pub fn meta(&self) -> Option<Meta> {
        match &self.state {
            StakeStateV2::Stake(meta, _stake, _stake_flags) => Some(meta.clone()),
            StakeStateV2::Initialized(meta) => Some(meta.clone()),
            StakeStateV2::Uninitialized | StakeStateV2::RewardsPool => None,
        }
    }

    pub fn meta_ref(&self) -> Option<&Meta> {
        match &self.state {
            StakeStateV2::Stake(meta, _stake, _stake_flags) => Some(meta),
            StakeStateV2::Initialized(meta) => Some(meta),
            StakeStateV2::Uninitialized | StakeStateV2::RewardsPool => None,
        }
    }
}

impl<'a> TryFrom<&solana_program::account_info::AccountInfo<'a>> for StakeStateAccount {
    type Error = std::io::Error;

    fn try_from(
        account_info: &solana_program::account_info::AccountInfo<'a>,
    ) -> Result<Self, Self::Error> {
        let mut data: &[u8] = &(*account_info.data).borrow();
        Self::deserialize(&mut data)
    }
}

impl BorshDeserialize for StakeStateAccount {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let enum_value: u32 = BorshDeserialize::deserialize_reader(reader)?;
        let state = match enum_value {
            0 => StakeStateV2::Uninitialized,
            1 => {
                let meta: Meta = BorshDeserialize::deserialize_reader(reader)?;
                StakeStateV2::Initialized(meta)
            }
            2 => {
                let meta: Meta = BorshDeserialize::deserialize_reader(reader)?;
                let stake: Stake = BorshDeserialize::deserialize_reader(reader)?;
                let stake_flags: StakeFlags = BorshDeserialize::deserialize_reader(reader)?;
                StakeStateV2::Stake(meta, stake, stake_flags)
            }
            3 => StakeStateV2::RewardsPool,
            _ => return Err(Error::new(ErrorKind::InvalidData, "Invalid enum value")),
        };

        Ok(StakeStateAccount { state })
    }
}

impl BorshSerialize for StakeStateAccount {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match self.state {
            StakeStateV2::Uninitialized => writer.write_all(&0u32.to_le_bytes()),
            StakeStateV2::Initialized(ref meta) => {
                writer.write_all(&1u32.to_le_bytes())?;
                BorshSerialize::serialize(meta, writer)
            }
            StakeStateV2::Stake(ref meta, ref stake, ref stake_flags) => {
                writer.write_all(&2u32.to_le_bytes())?;
                BorshSerialize::serialize(meta, writer)?;
                BorshSerialize::serialize(stake, writer)?;
                BorshSerialize::serialize(stake_flags, writer)
            }
            StakeStateV2::RewardsPool => writer.write_all(&3u32.to_le_bytes()),
        }
    }
}
