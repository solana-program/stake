use solana_program::{instruction::InstructionError, program_error::ProgramError};

pub(crate) mod delegate;
pub(crate) use delegate::*;

pub(crate) mod split;
pub(crate) use split::*;

pub(crate) mod merge;
pub(crate) use merge::*;

pub(crate) fn checked_add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(ProgramError::InsufficientFunds)
}

// FIXME this is kind of a hack... but better than mapping *all*
// InstructionError into ProgramError::InvalidAccountData idk if theres a more
// standard way
// XXX jon suggests an error wrapper like
// struct InfallibleInstructionError(InstructionError);
// impl From<InfallibleInstructionError> for ProgramError { ... }
// meta.authorized.authorize(...).map_err(|e|
// InfallibleInstructionError(e).into())?;
pub(crate) trait TurnInto {
    fn turn_into(self) -> ProgramError;
}

impl TurnInto for InstructionError {
    fn turn_into(self) -> ProgramError {
        match ProgramError::try_from(self) {
            Ok(program_error) => program_error,
            Err(_) => ProgramError::InvalidAccountData,
        }
    }
}
