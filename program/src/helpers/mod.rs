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

pub(crate) fn to_program_error(e: InstructionError) -> ProgramError {
    ProgramError::try_from(e).unwrap_or(ProgramError::InvalidAccountData)
}
