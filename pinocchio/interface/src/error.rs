use crate::state::StakeStateV2Tag;

#[derive(Debug)]
pub enum StakeStateError {
    /// The discriminant tag is not a valid variant (must be 0-3).
    InvalidTag(u32),
    /// An invalid state transition was attempted.
    InvalidTransition {
        from: StakeStateV2Tag,
        to: StakeStateV2Tag,
    },
    /// Pass-through for wincode read errors when borrowing layout structs.
    Read(wincode::ReadError),
    /// Input buffer is shorter than 200 bytes.
    UnexpectedEof,
    /// Pass-through for wincode write errors when serializing layout structs.
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
