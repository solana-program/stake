#![allow(dead_code)]
#![allow(unused_imports)]

use {
    crate::{feature_set_die, stake_history_die},
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::{Clock, Epoch},
        entrypoint::ProgramResult,
        instruction::{checked_add, InstructionError},
        msg,
        program_error::ProgramError,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        rent::Rent,
        stake::state::*,
        stake::{
            instruction::{LockupArgs, StakeError, StakeInstruction},
            program::id,
            stake_flags::StakeFlags,
            state::{Authorized, Lockup},
            tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
        },
        stake_history::{StakeHistory, StakeHistoryEntry},
        sysvar::Sysvar,
        vote::program as solana_vote_program,
        vote::state::{VoteState, VoteStateVersions},
    },
    std::{cmp::Ordering, collections::HashSet, convert::TryFrom},
};

pub struct Processor {}
impl Processor {
    fn process_initialize(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        authorized: Authorized,
        lockup: Lockup,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let rent_info = next_account_info(account_info_iter)?;
        let rent = &Rent::from_account_info(rent_info)?;

        if stake_account_info.data_len() != StakeStateV2::size_of() {
            // XXX this was a InstructionError... make sure its the same!!
            return Err(ProgramError::InvalidAccountData);
        }

        if let StakeStateV2::Uninitialized = stake_account_info.deserialize_data().unwrap() {
            let rent_exempt_reserve = rent.minimum_balance(stake_account_info.data_len());
            if stake_account_info.lamports() >= rent_exempt_reserve {
                let stake_state = StakeStateV2::Initialized(Meta {
                    rent_exempt_reserve,
                    authorized: authorized,
                    lockup: lockup,
                });

                bincode::serialize_into(
                    &mut stake_account_info.data.borrow_mut()[..],
                    &stake_state,
                )
                .unwrap();

                Ok(()) // XXX the above error as-written is InstructionError::GenericError
            } else {
                Err(ProgramError::InsufficientFunds)
            }
        } else {
            Err(ProgramError::InvalidAccountData)
        }?;

        Ok(())
    }

    /// Processes [Instruction](enum.Instruction.html).
    // XXX the existing program returns InstructionError not ProgramError
    // look into if theres a trait i can impl to not break the interface but modrenize
    pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
        let instruction = bincode::deserialize(data).unwrap(); // XXX limited_deserialize?
        msg!("HANA ixn: {:#?}", instruction);

        match instruction {
            StakeInstruction::Initialize(authorized, lockup) => {
                msg!("Instruction: Initialize");
                Self::process_initialize(program_id, accounts, authorized, lockup)
            }
            _ => unimplemented!(),
        }
    }
}
