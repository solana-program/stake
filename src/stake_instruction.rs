use {
    crate::stake_state::{
        authorize, authorize_with_seed, deactivate, deactivate_delinquent, delegate, initialize,
        merge, new_warmup_cooldown_rate_epoch, redelegate, set_lockup, split, withdraw,
    },
    solana_program::{
        instruction::InstructionError,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        stake::{
            instruction::{LockupArgs, StakeInstruction},
            program::id,
            state::{Authorized, Lockup},
        },
        transaction_context::{IndexOfAccount, InstructionContext, TransactionContext},
    },
    solana_program_runtime::declare_process_instruction,
};

fn get_optional_pubkey<'a>(
    transaction_context: &'a TransactionContext,
    instruction_context: &'a InstructionContext,
    instruction_account_index: IndexOfAccount,
    should_be_signer: bool,
) -> Result<Option<&'a Pubkey>, InstructionError> {
    Ok(
        if instruction_account_index < instruction_context.get_number_of_instruction_accounts() {
            if should_be_signer
                && !instruction_context.is_instruction_account_signer(instruction_account_index)?
            {
                return Err(InstructionError::MissingRequiredSignature);
            }
            Some(
                transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(
                        instruction_account_index,
                    )?,
                )?,
            )
        } else {
            None
        },
    )
}

pub const DEFAULT_COMPUTE_UNITS: u64 = 750;

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let data = instruction_context.get_instruction_data();

    trace!("process_instruction: {:?}", data);

    let get_stake_account = || {
        let me = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        if *me.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }
        Ok(me)
    };

    let signers = instruction_context.get_signers(transaction_context)?;
    match limited_deserialize(data) {
        Ok(StakeInstruction::Initialize(authorized, lockup)) => {
            let mut me = get_stake_account()?;
            initialize(&mut me, &authorized, &lockup)
        }
        Ok(StakeInstruction::Authorize(authorized_pubkey, stake_authorize)) => {
            let mut me = get_stake_account()?;
            let custodian_pubkey =
                get_optional_pubkey(transaction_context, instruction_context, 3, false)?;

            authorize(
                &mut me,
                &signers,
                &authorized_pubkey,
                stake_authorize,
                custodian_pubkey,
            )
        }
        Ok(StakeInstruction::AuthorizeWithSeed(args)) => {
            let mut me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            let custodian_pubkey =
                get_optional_pubkey(transaction_context, instruction_context, 3, false)?;

            authorize_with_seed(
                transaction_context,
                instruction_context,
                &mut me,
                1,
                &args.authority_seed,
                &args.authority_owner,
                &args.new_authorized_pubkey,
                args.stake_authorize,
                custodian_pubkey,
            )
        }
        Ok(StakeInstruction::DelegateStake) => {
            let me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            drop(me);
            if !crate::FEATURE_REDUCE_STAKE_WARMUP_COOLDOWN {
                // XXX REMOVED config check
            }
            delegate(transaction_context, instruction_context, 0, 1, &signers)
        }
        Ok(StakeInstruction::Split(lamports)) => {
            let me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            drop(me);
            split(
                transaction_context,
                instruction_context,
                0,
                lamports,
                1,
                &signers,
            )
        }
        Ok(StakeInstruction::Merge) => {
            let me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            drop(me);
            merge(transaction_context, instruction_context, 0, 1, &signers)
        }
        Ok(StakeInstruction::Withdraw(lamports)) => {
            let me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            drop(me);
            withdraw(
                transaction_context,
                instruction_context,
                0,
                lamports,
                1,
                4,
                // XXX this is wrong now (we will delete all this anyway)
                if instruction_context.get_number_of_instruction_accounts() >= 6 {
                    Some(5)
                } else {
                    None
                },
                new_warmup_cooldown_rate_epoch(),
            )
        }
        Ok(StakeInstruction::Deactivate) => {
            let mut me = get_stake_account()?;
            deactivate(&mut me, &signers)
        }
        Ok(StakeInstruction::SetLockup(lockup)) => {
            let mut me = get_stake_account()?;
            let clock = invoke_context.get_sysvar_cache().get_clock()?;
            set_lockup(&mut me, &lockup, &signers, &clock)
        }
        Ok(StakeInstruction::InitializeChecked) => {
            let mut me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(4)?;
            let staker_pubkey = transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(2)?,
            )?;
            let withdrawer_pubkey = transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(3)?,
            )?;
            if !instruction_context.is_instruction_account_signer(3)? {
                return Err(InstructionError::MissingRequiredSignature);
            }

            let authorized = Authorized {
                staker: *staker_pubkey,
                withdrawer: *withdrawer_pubkey,
            };

            initialize(&mut me, &authorized, &Lockup::default())
        }
        Ok(StakeInstruction::AuthorizeChecked(stake_authorize)) => {
            let mut me = get_stake_account()?;
            // XXX these indexes wrong too
            let authorized_pubkey = transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(3)?,
            )?;
            if !instruction_context.is_instruction_account_signer(3)? {
                return Err(InstructionError::MissingRequiredSignature);
            }
            let custodian_pubkey =
                get_optional_pubkey(transaction_context, instruction_context, 4, false)?;

            authorize(
                &mut me,
                &signers,
                authorized_pubkey,
                stake_authorize,
                custodian_pubkey,
            )
        }
        Ok(StakeInstruction::AuthorizeCheckedWithSeed(args)) => {
            let mut me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            // XXX wrong too
            let authorized_pubkey = transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(3)?,
            )?;
            if !instruction_context.is_instruction_account_signer(3)? {
                return Err(InstructionError::MissingRequiredSignature);
            }
            let custodian_pubkey =
                get_optional_pubkey(transaction_context, instruction_context, 4, false)?;

            authorize_with_seed(
                transaction_context,
                instruction_context,
                &mut me,
                1,
                &args.authority_seed,
                &args.authority_owner,
                authorized_pubkey,
                args.stake_authorize,
                custodian_pubkey,
            )
        }
        Ok(StakeInstruction::SetLockupChecked(lockup_checked)) => {
            let mut me = get_stake_account()?;
            let custodian_pubkey =
                get_optional_pubkey(transaction_context, instruction_context, 2, true)?;

            let lockup = LockupArgs {
                unix_timestamp: lockup_checked.unix_timestamp,
                epoch: lockup_checked.epoch,
                custodian: custodian_pubkey.cloned(),
            };
            let clock = invoke_context.get_sysvar_cache().get_clock()?;
            set_lockup(&mut me, &lockup, &signers, &clock)
        }
        Ok(StakeInstruction::GetMinimumDelegation) => {
            let minimum_delegation = crate::get_minimum_delegation();
            let minimum_delegation = Vec::from(minimum_delegation.to_le_bytes());
            invoke_context
                .transaction_context
                .set_return_data(id(), minimum_delegation)
        }
        Ok(StakeInstruction::DeactivateDelinquent) => {
            let mut me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(3)?;

            let clock = invoke_context.get_sysvar_cache().get_clock()?;
            deactivate_delinquent(
                transaction_context,
                instruction_context,
                &mut me,
                1,
                2,
                clock.epoch,
            )
        }
        Ok(StakeInstruction::Redelegate) => {
            let mut me = get_stake_account()?;
            if FEATURE_STAKE_REDELEGATE_INSTRUCTION {
                instruction_context.check_number_of_instruction_accounts(3)?;
                if FEATURE_REDUCE_STAKE_WARMUP_COOLDOWN {
                    // XXX REMOVED config check
                }
                redelegate(
                    transaction_context,
                    instruction_context,
                    &mut me,
                    1,
                    2,
                    &signers,
                )
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
        Err(err) => Err(err),
    }
});
