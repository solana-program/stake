//! Program entrypoint

#![cfg(all(target_os = "solana", not(feature = "no-entrypoint")))]

use {
    crate::omnibus::Processor,
    solana_program::{
        account_info::AccountInfo, entrypoint::ProgramResult, program_error::PrintProgramError,
        pubkey::Pubkey,
    },
};

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // XXX print error once we have proper errors
    Processor::process(program_id, accounts, instruction_data)
}
