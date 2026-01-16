use crate::state::StakeStateV2Tag;

#[derive(Debug)]
pub enum StakeStateError {
    /// Field access invalid for the current state.
    InvalidStateAccess(StakeStateV2Tag),
    /// Tag is not a valid variant (0-3).
    InvalidTag(u32),
    /// Invalid state transition attempted.
    InvalidTransition {
        from: StakeStateV2Tag,
        to: StakeStateV2Tag,
    },
    /// Wincode deserialization error.
    Read(wincode::ReadError),
    /// Buffer shorter than 200 bytes.
    UnexpectedEof,
    /// Wincode serialization error.
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
