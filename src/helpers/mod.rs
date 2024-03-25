use {
    num_traits::cast::ToPrimitive,
    solana_program::{
        instruction::InstructionError, program_error::ProgramError, stake::instruction::StakeError,
    },
};

pub(crate) mod delegate;
pub(crate) use delegate::*;

pub(crate) mod split;
pub(crate) use split::*;

pub(crate) mod merge;
pub(crate) use merge::*;

pub(crate) fn checked_add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(ProgramError::InsufficientFunds)
}

// XXX impl from<StakeError> for ProgramError. also idk if this is correct
// i just want to keep the same errors in-place and then clean up later, instead of needing to hunt down the right ones
// XXX there should also be a better wrapper for TryFrom<InstructionError> for ProgramError
// like, if theres a matching error do the conversion, if custom do the custom conversion
// otherwise unwrap into an error cnoversion error maybe. idk

// TODO pr monorepo to imple From<StakeError> for ProgramError, make sure this is how uhhh
// impl From<SinglePoolError> for ProgramError {
//     fn from(e: SinglePoolError) -> Self {
//         ProgramError::Custom(e as u32)
//     }
// }
// ok looks fine lol. also figure out a better way for instruction error conversion its so annoying
pub(crate) trait TurnInto {
    fn turn_into(self) -> ProgramError;
}

impl TurnInto for StakeError {
    fn turn_into(self) -> ProgramError {
        ProgramError::Custom(self.to_u32().unwrap())
    }
}

impl TurnInto for InstructionError {
    fn turn_into(self) -> ProgramError {
        match ProgramError::try_from(self) {
            Ok(program_error) => program_error,
            Err(e) => panic!("HANA error conversion failed: {:?}", e),
        }
    }
}
