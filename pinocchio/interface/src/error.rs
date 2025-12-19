// TODO: These may not be backward compatible with StakeStakeV2 serialization errs in interface/src/state.rs
//       Need to think about this.
#[derive(Debug)]
pub enum StakeStateError {
    WrongLength { expected: usize, actual: usize },
    Read(wincode::ReadError),
}

impl From<wincode::ReadError> for StakeStateError {
    #[inline(always)]
    fn from(e: wincode::ReadError) -> Self {
        Self::Read(e)
    }
}

#[inline(always)]
pub(crate) fn invalid_tag(tag: u32) -> StakeStateError {
    StakeStateError::Read(wincode::error::invalid_tag_encoding(tag as usize))
}

#[inline(always)]
pub(crate) fn slice_as_array<const N: usize>(s: &[u8]) -> Result<&[u8; N], StakeStateError> {
    s.try_into().map_err(|_| {
        StakeStateError::Read(wincode::ReadError::Custom(
            "slice length mismatch for array",
        ))
    })
}

#[inline(always)]
pub(crate) fn slice_as_array_mut<const N: usize>(
    s: &mut [u8],
) -> Result<&mut [u8; N], StakeStateError> {
    s.try_into().map_err(|_| {
        StakeStateError::Read(wincode::ReadError::Custom(
            "slice length mismatch for array",
        ))
    })
}
