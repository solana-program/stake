use crate::state::StakeStateV2Tag;

#[derive(Debug)]
pub enum StakeStateError {
    /// Failed to decode stake state from bytes.
    Decode,
    /// Field access invalid for the current state.
    InvalidStateAccess(StakeStateV2Tag),
    /// Tag is not a valid variant (0-3).
    InvalidTag(u32),
    /// Invalid state transition attempted.
    InvalidTransition {
        from: StakeStateV2Tag,
        to: StakeStateV2Tag,
    },
    /// Buffer shorter than 200 bytes.
    UnexpectedEof,
}
