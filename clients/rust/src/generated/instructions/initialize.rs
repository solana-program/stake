//! This code was AUTOGENERATED using the codama library.
//! Please DO NOT EDIT THIS FILE, instead use visitors
//! to add features, then rerun codama to update it.
//!
//! <https://github.com/codama-idl/codama>
//!

use {
    crate::generated::types::{Authorized, Lockup},
    borsh::{BorshDeserialize, BorshSerialize},
};

/// Accounts.
#[derive(Debug)]
pub struct Initialize {
    /// Uninitialized stake account
    pub stake: solana_program::pubkey::Pubkey,
    /// Rent sysvar
    pub rent_sysvar: solana_program::pubkey::Pubkey,
}

impl Initialize {
    pub fn instruction(
        &self,
        args: InitializeInstructionArgs,
    ) -> solana_program::instruction::Instruction {
        self.instruction_with_remaining_accounts(args, &[])
    }
    #[allow(clippy::vec_init_then_push)]
    pub fn instruction_with_remaining_accounts(
        &self,
        args: InitializeInstructionArgs,
        remaining_accounts: &[solana_program::instruction::AccountMeta],
    ) -> solana_program::instruction::Instruction {
        let mut accounts = Vec::with_capacity(2 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.stake, false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.rent_sysvar,
            false,
        ));
        accounts.extend_from_slice(remaining_accounts);
        let mut data = borsh::to_vec(&InitializeInstructionData::new()).unwrap();
        let mut args = borsh::to_vec(&args).unwrap();
        data.append(&mut args);

        solana_program::instruction::Instruction {
            program_id: crate::STAKE_ID,
            accounts,
            data,
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InitializeInstructionData {
    discriminator: u32,
}

impl InitializeInstructionData {
    pub fn new() -> Self {
        Self { discriminator: 0 }
    }
}

impl Default for InitializeInstructionData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InitializeInstructionArgs {
    pub arg0: Authorized,
    pub arg1: Lockup,
}

/// Instruction builder for `Initialize`.
///
/// ### Accounts:
///
///   0. `[writable]` stake
///   1. `[optional]` rent_sysvar (default to `SysvarRent111111111111111111111111111111111`)
#[derive(Clone, Debug, Default)]
pub struct InitializeBuilder {
    stake: Option<solana_program::pubkey::Pubkey>,
    rent_sysvar: Option<solana_program::pubkey::Pubkey>,
    arg0: Option<Authorized>,
    arg1: Option<Lockup>,
    __remaining_accounts: Vec<solana_program::instruction::AccountMeta>,
}

impl InitializeBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    /// Uninitialized stake account
    #[inline(always)]
    pub fn stake(&mut self, stake: solana_program::pubkey::Pubkey) -> &mut Self {
        self.stake = Some(stake);
        self
    }
    /// `[optional account, default to 'SysvarRent111111111111111111111111111111111']`
    /// Rent sysvar
    #[inline(always)]
    pub fn rent_sysvar(&mut self, rent_sysvar: solana_program::pubkey::Pubkey) -> &mut Self {
        self.rent_sysvar = Some(rent_sysvar);
        self
    }
    #[inline(always)]
    pub fn arg0(&mut self, arg0: Authorized) -> &mut Self {
        self.arg0 = Some(arg0);
        self
    }
    #[inline(always)]
    pub fn arg1(&mut self, arg1: Lockup) -> &mut Self {
        self.arg1 = Some(arg1);
        self
    }
    /// Add an additional account to the instruction.
    #[inline(always)]
    pub fn add_remaining_account(
        &mut self,
        account: solana_program::instruction::AccountMeta,
    ) -> &mut Self {
        self.__remaining_accounts.push(account);
        self
    }
    /// Add additional accounts to the instruction.
    #[inline(always)]
    pub fn add_remaining_accounts(
        &mut self,
        accounts: &[solana_program::instruction::AccountMeta],
    ) -> &mut Self {
        self.__remaining_accounts.extend_from_slice(accounts);
        self
    }
    #[allow(clippy::clone_on_copy)]
    pub fn instruction(&self) -> solana_program::instruction::Instruction {
        let accounts = Initialize {
            stake: self.stake.expect("stake is not set"),
            rent_sysvar: self.rent_sysvar.unwrap_or(solana_program::pubkey!(
                "SysvarRent111111111111111111111111111111111"
            )),
        };
        let args = InitializeInstructionArgs {
            arg0: self.arg0.clone().expect("arg0 is not set"),
            arg1: self.arg1.clone().expect("arg1 is not set"),
        };

        accounts.instruction_with_remaining_accounts(args, &self.__remaining_accounts)
    }
}

/// `initialize` CPI accounts.
pub struct InitializeCpiAccounts<'a, 'b> {
    /// Uninitialized stake account
    pub stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Rent sysvar
    pub rent_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
}

/// `initialize` CPI instruction.
pub struct InitializeCpi<'a, 'b> {
    /// The program to invoke.
    pub __program: &'b solana_program::account_info::AccountInfo<'a>,
    /// Uninitialized stake account
    pub stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Rent sysvar
    pub rent_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
    /// The arguments for the instruction.
    pub __args: InitializeInstructionArgs,
}

impl<'a, 'b> InitializeCpi<'a, 'b> {
    pub fn new(
        program: &'b solana_program::account_info::AccountInfo<'a>,
        accounts: InitializeCpiAccounts<'a, 'b>,
        args: InitializeInstructionArgs,
    ) -> Self {
        Self {
            __program: program,
            stake: accounts.stake,
            rent_sysvar: accounts.rent_sysvar,
            __args: args,
        }
    }
    #[inline(always)]
    pub fn invoke(&self) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed_with_remaining_accounts(&[], &[])
    }
    #[inline(always)]
    pub fn invoke_with_remaining_accounts(
        &self,
        remaining_accounts: &[(
            &'b solana_program::account_info::AccountInfo<'a>,
            bool,
            bool,
        )],
    ) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed_with_remaining_accounts(&[], remaining_accounts)
    }
    #[inline(always)]
    pub fn invoke_signed(
        &self,
        signers_seeds: &[&[&[u8]]],
    ) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed_with_remaining_accounts(signers_seeds, &[])
    }
    #[allow(clippy::clone_on_copy)]
    #[allow(clippy::vec_init_then_push)]
    pub fn invoke_signed_with_remaining_accounts(
        &self,
        signers_seeds: &[&[&[u8]]],
        remaining_accounts: &[(
            &'b solana_program::account_info::AccountInfo<'a>,
            bool,
            bool,
        )],
    ) -> solana_program::entrypoint::ProgramResult {
        let mut accounts = Vec::with_capacity(2 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.stake.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.rent_sysvar.key,
            false,
        ));
        remaining_accounts.iter().for_each(|remaining_account| {
            accounts.push(solana_program::instruction::AccountMeta {
                pubkey: *remaining_account.0.key,
                is_signer: remaining_account.1,
                is_writable: remaining_account.2,
            })
        });
        let mut data = borsh::to_vec(&InitializeInstructionData::new()).unwrap();
        let mut args = borsh::to_vec(&self.__args).unwrap();
        data.append(&mut args);

        let instruction = solana_program::instruction::Instruction {
            program_id: crate::STAKE_ID,
            accounts,
            data,
        };
        let mut account_infos = Vec::with_capacity(3 + remaining_accounts.len());
        account_infos.push(self.__program.clone());
        account_infos.push(self.stake.clone());
        account_infos.push(self.rent_sysvar.clone());
        remaining_accounts
            .iter()
            .for_each(|remaining_account| account_infos.push(remaining_account.0.clone()));

        if signers_seeds.is_empty() {
            solana_program::program::invoke(&instruction, &account_infos)
        } else {
            solana_program::program::invoke_signed(&instruction, &account_infos, signers_seeds)
        }
    }
}

/// Instruction builder for `Initialize` via CPI.
///
/// ### Accounts:
///
///   0. `[writable]` stake
///   1. `[]` rent_sysvar
#[derive(Clone, Debug)]
pub struct InitializeCpiBuilder<'a, 'b> {
    instruction: Box<InitializeCpiBuilderInstruction<'a, 'b>>,
}

impl<'a, 'b> InitializeCpiBuilder<'a, 'b> {
    pub fn new(program: &'b solana_program::account_info::AccountInfo<'a>) -> Self {
        let instruction = Box::new(InitializeCpiBuilderInstruction {
            __program: program,
            stake: None,
            rent_sysvar: None,
            arg0: None,
            arg1: None,
            __remaining_accounts: Vec::new(),
        });
        Self { instruction }
    }
    /// Uninitialized stake account
    #[inline(always)]
    pub fn stake(&mut self, stake: &'b solana_program::account_info::AccountInfo<'a>) -> &mut Self {
        self.instruction.stake = Some(stake);
        self
    }
    /// Rent sysvar
    #[inline(always)]
    pub fn rent_sysvar(
        &mut self,
        rent_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.rent_sysvar = Some(rent_sysvar);
        self
    }
    #[inline(always)]
    pub fn arg0(&mut self, arg0: Authorized) -> &mut Self {
        self.instruction.arg0 = Some(arg0);
        self
    }
    #[inline(always)]
    pub fn arg1(&mut self, arg1: Lockup) -> &mut Self {
        self.instruction.arg1 = Some(arg1);
        self
    }
    /// Add an additional account to the instruction.
    #[inline(always)]
    pub fn add_remaining_account(
        &mut self,
        account: &'b solana_program::account_info::AccountInfo<'a>,
        is_writable: bool,
        is_signer: bool,
    ) -> &mut Self {
        self.instruction
            .__remaining_accounts
            .push((account, is_writable, is_signer));
        self
    }
    /// Add additional accounts to the instruction.
    ///
    /// Each account is represented by a tuple of the `AccountInfo`, a `bool` indicating whether the account is writable or not,
    /// and a `bool` indicating whether the account is a signer or not.
    #[inline(always)]
    pub fn add_remaining_accounts(
        &mut self,
        accounts: &[(
            &'b solana_program::account_info::AccountInfo<'a>,
            bool,
            bool,
        )],
    ) -> &mut Self {
        self.instruction
            .__remaining_accounts
            .extend_from_slice(accounts);
        self
    }
    #[inline(always)]
    pub fn invoke(&self) -> solana_program::entrypoint::ProgramResult {
        self.invoke_signed(&[])
    }
    #[allow(clippy::clone_on_copy)]
    #[allow(clippy::vec_init_then_push)]
    pub fn invoke_signed(
        &self,
        signers_seeds: &[&[&[u8]]],
    ) -> solana_program::entrypoint::ProgramResult {
        let args = InitializeInstructionArgs {
            arg0: self.instruction.arg0.clone().expect("arg0 is not set"),
            arg1: self.instruction.arg1.clone().expect("arg1 is not set"),
        };
        let instruction = InitializeCpi {
            __program: self.instruction.__program,

            stake: self.instruction.stake.expect("stake is not set"),

            rent_sysvar: self
                .instruction
                .rent_sysvar
                .expect("rent_sysvar is not set"),
            __args: args,
        };
        instruction.invoke_signed_with_remaining_accounts(
            signers_seeds,
            &self.instruction.__remaining_accounts,
        )
    }
}

#[derive(Clone, Debug)]
struct InitializeCpiBuilderInstruction<'a, 'b> {
    __program: &'b solana_program::account_info::AccountInfo<'a>,
    stake: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    rent_sysvar: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    arg0: Option<Authorized>,
    arg1: Option<Lockup>,
    /// Additional instruction accounts `(AccountInfo, is_writable, is_signer)`.
    __remaining_accounts: Vec<(
        &'b solana_program::account_info::AccountInfo<'a>,
        bool,
        bool,
    )>,
}
