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

// XXX note to self. InstructionError is actually a superset of ProgramError
// there is a TryFrom instance, but thats why theres no From instance
// there are ProgramError conversions between u64 tho, and From<T> for InstructionError where T: FromPrimitive
// very unusual. i guess i can look more into this but for now using ProgramError is fine seems safe

// XXX a nice change would be to pop an account off the queue and discard if its a gettable sysvar
// ie, allow people to omit them from the accounts list without breaking compat

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

    fn process_delegate(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let vote_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let stake_history_info = next_account_info(account_info_iter)?;
        let _stake_config_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        if *vote_account_info.owner != solana_vote_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        //let mut vote_state = Box::new(VoteState::default());
        //VoteState::deserialize_into(&vote_account_info.data.borrow(), &mut vote_state).unwrap();
        //let vote_state = vote_state;
        let vote_state = VoteState::deserialize(&vote_account_info.data.borrow()).unwrap();

        // XXX parse stake account, branch on enum, new stake or redelegate

        Ok(())
    }

    /// Processes [Instruction](enum.Instruction.html).
    // XXX the existing program returns InstructionError not ProgramError
    // look into if theres a trait i can impl to not break the interface but modrenize
    pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
        let instruction = bincode::deserialize(data).unwrap(); // XXX limited_deserialize?

        match instruction {
            StakeInstruction::Initialize(authorized, lockup) => {
                msg!("Instruction: Initialize");
                Self::process_initialize(program_id, accounts, authorized, lockup)
            }
            StakeInstruction::DelegateStake => {
                msg!("Instruction: DelegateStake");

                if !crate::FEATURE_REDUCE_STAKE_WARMUP_COOLDOWN {
                    panic!("we only impl the `reduce_stake_warmup_cooldown` logic");
                }

                Self::process_delegate(program_id, accounts)
            }
            _ => unimplemented!(),
        }
    }
}
