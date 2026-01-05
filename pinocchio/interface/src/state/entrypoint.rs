use {
    super::{view::StakeStateV2View, writer::StakeStateV2Writer},
    crate::error::StakeStateError,
};

pub struct StakeStateV2;

impl StakeStateV2 {
    /// Parse stake account data into a read-only view.
    pub fn from_bytes(data: &[u8]) -> Result<StakeStateV2View<'_>, StakeStateError> {
        StakeStateV2View::from_bytes(data)
    }

    /// Parse stake account data into a mutable writer.
    pub fn from_bytes_mut(data: &mut [u8]) -> Result<StakeStateV2Writer<'_>, StakeStateError> {
        StakeStateV2Writer::from_bytes_mut(data)
    }
}
