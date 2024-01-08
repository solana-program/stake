use {
    crate::{
        config,
        stake_state::{
            authorize, authorize_with_seed, deactivate, deactivate_delinquent, delegate,
            initialize, merge, new_warmup_cooldown_rate_epoch, redelegate, set_lockup, split,
            withdraw,
        },
    },
    log::*,
    solana_program_runtime::{
        declare_process_instruction, sysvar_cache::get_sysvar_with_account_check,
    },
    solana_sdk::{
        feature_set,
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
            let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 1)?;
            initialize(
                &mut me,
                &authorized,
                &lockup,
                &rent,
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::Authorize(authorized_pubkey, stake_authorize)) => {
            let mut me = get_stake_account()?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 1)?;
            instruction_context.check_number_of_instruction_accounts(3)?;
            let custodian_pubkey =
                get_optional_pubkey(transaction_context, instruction_context, 3, false)?;

            authorize(
                &mut me,
                &signers,
                &authorized_pubkey,
                stake_authorize,
                &clock,
                custodian_pubkey,
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::AuthorizeWithSeed(args)) => {
            let mut me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
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
                &clock,
                custodian_pubkey,
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::DelegateStake) => {
            let me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                invoke_context,
                instruction_context,
                3,
            )?;
            instruction_context.check_number_of_instruction_accounts(5)?;
            drop(me);
            if !invoke_context
                .feature_set
                .is_active(&feature_set::reduce_stake_warmup_cooldown::id())
            {
                // Post feature activation, remove both the feature gate code and the config completely in the interface
                let config_account =
                    instruction_context.try_borrow_instruction_account(transaction_context, 4)?;
                #[allow(deprecated)]
                if !config::check_id(config_account.get_key()) {
                    return Err(InstructionError::InvalidArgument);
                }
                config::from(&config_account).ok_or(InstructionError::InvalidArgument)?;
            }
            delegate(
                invoke_context,
                transaction_context,
                instruction_context,
                0,
                1,
                &clock,
                &stake_history,
                &signers,
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::Split(lamports)) => {
            let me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            drop(me);
            split(
                invoke_context,
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
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                invoke_context,
                instruction_context,
                3,
            )?;
            drop(me);
            merge(
                invoke_context,
                transaction_context,
                instruction_context,
                0,
                1,
                &clock,
                &stake_history,
                &signers,
            )
        }
        Ok(StakeInstruction::Withdraw(lamports)) => {
            let me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                invoke_context,
                instruction_context,
                3,
            )?;
            instruction_context.check_number_of_instruction_accounts(5)?;
            drop(me);
            withdraw(
                transaction_context,
                instruction_context,
                0,
                lamports,
                1,
                &clock,
                &stake_history,
                4,
                if instruction_context.get_number_of_instruction_accounts() >= 6 {
                    Some(5)
                } else {
                    None
                },
                new_warmup_cooldown_rate_epoch(invoke_context),
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::Deactivate) => {
            let mut me = get_stake_account()?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 1)?;
            deactivate(invoke_context, &mut me, &clock, &signers)
        }
        Ok(StakeInstruction::SetLockup(lockup)) => {
            let mut me = get_stake_account()?;
            let clock = invoke_context.get_sysvar_cache().get_clock()?;
            set_lockup(
                &mut me,
                &lockup,
                &signers,
                &clock,
                &invoke_context.feature_set,
            )
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

            let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 1)?;
            initialize(
                &mut me,
                &authorized,
                &Lockup::default(),
                &rent,
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::AuthorizeChecked(stake_authorize)) => {
            let mut me = get_stake_account()?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 1)?;
            instruction_context.check_number_of_instruction_accounts(4)?;
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
                &clock,
                custodian_pubkey,
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::AuthorizeCheckedWithSeed(args)) => {
            let mut me = get_stake_account()?;
            instruction_context.check_number_of_instruction_accounts(2)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
            instruction_context.check_number_of_instruction_accounts(4)?;
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
                &clock,
                custodian_pubkey,
                &invoke_context.feature_set,
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
            set_lockup(
                &mut me,
                &lockup,
                &signers,
                &clock,
                &invoke_context.feature_set,
            )
        }
        Ok(StakeInstruction::GetMinimumDelegation) => {
            let feature_set = invoke_context.feature_set.as_ref();
            let minimum_delegation = crate::get_minimum_delegation(feature_set);
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
                invoke_context,
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
            if invoke_context
                .feature_set
                .is_active(&feature_set::stake_redelegate_instruction::id())
            {
                instruction_context.check_number_of_instruction_accounts(3)?;
                if !invoke_context
                    .feature_set
                    .is_active(&feature_set::reduce_stake_warmup_cooldown::id())
                {
                    // Post feature activation, remove both the feature gate code and the config completely in the interface
                    let config_account = instruction_context
                        .try_borrow_instruction_account(transaction_context, 3)?;
                    #[allow(deprecated)]
                    if !config::check_id(config_account.get_key()) {
                        return Err(InstructionError::InvalidArgument);
                    }
                    config::from(&config_account).ok_or(InstructionError::InvalidArgument)?;
                }
                redelegate(
                    invoke_context,
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
