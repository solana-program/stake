//! Error types for stake state parsing.

use crate::state::StakeStateV2Tag;

#[derive(Debug)]
pub enum StakeStateError {
    /// Input is shorter than 200 bytes.
    UnexpectedEof,

    /// The discriminant tag is not a valid variant
    InvalidTag(u32),

    /// An invalid state transition was attempted.
    InvalidTransition {
        from: StakeStateV2Tag,
        to: StakeStateV2Tag,
    },

    /// Pass-through for wincode read errors.
    Read(wincode::ReadError),

    /// Pass-through for wincode write errors.
    Write(wincode::WriteError),
}

impl From<wincode::ReadError> for StakeStateError {
    fn from(e: wincode::ReadError) -> Self {
        StakeStateError::Read(e)
    }
}

impl From<wincode::WriteError> for StakeStateError {
    fn from(e: wincode::WriteError) -> Self {
        StakeStateError::Write(e)
    }
}
