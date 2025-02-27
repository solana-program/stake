//! This code was AUTOGENERATED using the codama library.
//! Please DO NOT EDIT THIS FILE, instead use visitors
//! to add features, then rerun codama to update it.
//!
//! <https://github.com/codama-idl/codama>
//!

use borsh::{BorshDeserialize, BorshSerialize};

/// Accounts.
pub struct SetLockupChecked {
    /// Initialized stake account
    pub stake: solana_program::pubkey::Pubkey,
    /// Lockup authority or withdraw authority
    pub authority: solana_program::pubkey::Pubkey,
    /// New lockup authority
    pub new_authority: Option<solana_program::pubkey::Pubkey>,
}

impl SetLockupChecked {
    pub fn instruction(
        &self,
        args: SetLockupCheckedInstructionArgs,
    ) -> solana_program::instruction::Instruction {
        self.instruction_with_remaining_accounts(args, &[])
    }
    #[allow(clippy::vec_init_then_push)]
    pub fn instruction_with_remaining_accounts(
        &self,
        args: SetLockupCheckedInstructionArgs,
        remaining_accounts: &[solana_program::instruction::AccountMeta],
    ) -> solana_program::instruction::Instruction {
        let mut accounts = Vec::with_capacity(3 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.stake, false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.authority,
            true,
        ));
        if let Some(new_authority) = self.new_authority {
            accounts.push(solana_program::instruction::AccountMeta::new_readonly(
                new_authority,
                true,
            ));
        } else {
            accounts.push(solana_program::instruction::AccountMeta::new_readonly(
                crate::STAKE_ID,
                false,
            ));
        }
        accounts.extend_from_slice(remaining_accounts);
        let mut data = SetLockupCheckedInstructionData::new().try_to_vec().unwrap();
        let mut args = args.try_to_vec().unwrap();
        data.append(&mut args);

        solana_program::instruction::Instruction {
            program_id: crate::STAKE_ID,
            accounts,
            data,
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SetLockupCheckedInstructionData {
    discriminator: u32,
}

impl SetLockupCheckedInstructionData {
    pub fn new() -> Self {
        Self { discriminator: 12 }
    }
}

impl Default for SetLockupCheckedInstructionData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SetLockupCheckedInstructionArgs {
    pub unix_timestamp: Option<i64>,
    pub epoch: Option<u64>,
}

/// Instruction builder for `SetLockupChecked`.
///
/// ### Accounts:
///
///   0. `[writable]` stake
///   1. `[signer]` authority
///   2. `[signer, optional]` new_authority
#[derive(Clone, Debug, Default)]
pub struct SetLockupCheckedBuilder {
    stake: Option<solana_program::pubkey::Pubkey>,
    authority: Option<solana_program::pubkey::Pubkey>,
    new_authority: Option<solana_program::pubkey::Pubkey>,
    unix_timestamp: Option<i64>,
    epoch: Option<u64>,
    __remaining_accounts: Vec<solana_program::instruction::AccountMeta>,
}

impl SetLockupCheckedBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    /// Initialized stake account
    #[inline(always)]
    pub fn stake(&mut self, stake: solana_program::pubkey::Pubkey) -> &mut Self {
        self.stake = Some(stake);
        self
    }
    /// Lockup authority or withdraw authority
    #[inline(always)]
    pub fn authority(&mut self, authority: solana_program::pubkey::Pubkey) -> &mut Self {
        self.authority = Some(authority);
        self
    }
    /// `[optional account]`
    /// New lockup authority
    #[inline(always)]
    pub fn new_authority(
        &mut self,
        new_authority: Option<solana_program::pubkey::Pubkey>,
    ) -> &mut Self {
        self.new_authority = new_authority;
        self
    }
    /// `[optional argument]`
    #[inline(always)]
    pub fn unix_timestamp(&mut self, unix_timestamp: i64) -> &mut Self {
        self.unix_timestamp = Some(unix_timestamp);
        self
    }
    /// `[optional argument]`
    #[inline(always)]
    pub fn epoch(&mut self, epoch: u64) -> &mut Self {
        self.epoch = Some(epoch);
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
        let accounts = SetLockupChecked {
            stake: self.stake.expect("stake is not set"),
            authority: self.authority.expect("authority is not set"),
            new_authority: self.new_authority,
        };
        let args = SetLockupCheckedInstructionArgs {
            unix_timestamp: self.unix_timestamp.clone(),
            epoch: self.epoch.clone(),
        };

        accounts.instruction_with_remaining_accounts(args, &self.__remaining_accounts)
    }
}

/// `set_lockup_checked` CPI accounts.
pub struct SetLockupCheckedCpiAccounts<'a, 'b> {
    /// Initialized stake account
    pub stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Lockup authority or withdraw authority
    pub authority: &'b solana_program::account_info::AccountInfo<'a>,
    /// New lockup authority
    pub new_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
}

/// `set_lockup_checked` CPI instruction.
pub struct SetLockupCheckedCpi<'a, 'b> {
    /// The program to invoke.
    pub __program: &'b solana_program::account_info::AccountInfo<'a>,
    /// Initialized stake account
    pub stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Lockup authority or withdraw authority
    pub authority: &'b solana_program::account_info::AccountInfo<'a>,
    /// New lockup authority
    pub new_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    /// The arguments for the instruction.
    pub __args: SetLockupCheckedInstructionArgs,
}

impl<'a, 'b> SetLockupCheckedCpi<'a, 'b> {
    pub fn new(
        program: &'b solana_program::account_info::AccountInfo<'a>,
        accounts: SetLockupCheckedCpiAccounts<'a, 'b>,
        args: SetLockupCheckedInstructionArgs,
    ) -> Self {
        Self {
            __program: program,
            stake: accounts.stake,
            authority: accounts.authority,
            new_authority: accounts.new_authority,
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
        let mut accounts = Vec::with_capacity(3 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.stake.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.authority.key,
            true,
        ));
        if let Some(new_authority) = self.new_authority {
            accounts.push(solana_program::instruction::AccountMeta::new_readonly(
                *new_authority.key,
                true,
            ));
        } else {
            accounts.push(solana_program::instruction::AccountMeta::new_readonly(
                crate::STAKE_ID,
                false,
            ));
        }
        remaining_accounts.iter().for_each(|remaining_account| {
            accounts.push(solana_program::instruction::AccountMeta {
                pubkey: *remaining_account.0.key,
                is_signer: remaining_account.1,
                is_writable: remaining_account.2,
            })
        });
        let mut data = SetLockupCheckedInstructionData::new().try_to_vec().unwrap();
        let mut args = self.__args.try_to_vec().unwrap();
        data.append(&mut args);

        let instruction = solana_program::instruction::Instruction {
            program_id: crate::STAKE_ID,
            accounts,
            data,
        };
        let mut account_infos = Vec::with_capacity(4 + remaining_accounts.len());
        account_infos.push(self.__program.clone());
        account_infos.push(self.stake.clone());
        account_infos.push(self.authority.clone());
        if let Some(new_authority) = self.new_authority {
            account_infos.push(new_authority.clone());
        }
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

/// Instruction builder for `SetLockupChecked` via CPI.
///
/// ### Accounts:
///
///   0. `[writable]` stake
///   1. `[signer]` authority
///   2. `[signer, optional]` new_authority
#[derive(Clone, Debug)]
pub struct SetLockupCheckedCpiBuilder<'a, 'b> {
    instruction: Box<SetLockupCheckedCpiBuilderInstruction<'a, 'b>>,
}

impl<'a, 'b> SetLockupCheckedCpiBuilder<'a, 'b> {
    pub fn new(program: &'b solana_program::account_info::AccountInfo<'a>) -> Self {
        let instruction = Box::new(SetLockupCheckedCpiBuilderInstruction {
            __program: program,
            stake: None,
            authority: None,
            new_authority: None,
            unix_timestamp: None,
            epoch: None,
            __remaining_accounts: Vec::new(),
        });
        Self { instruction }
    }
    /// Initialized stake account
    #[inline(always)]
    pub fn stake(&mut self, stake: &'b solana_program::account_info::AccountInfo<'a>) -> &mut Self {
        self.instruction.stake = Some(stake);
        self
    }
    /// Lockup authority or withdraw authority
    #[inline(always)]
    pub fn authority(
        &mut self,
        authority: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.authority = Some(authority);
        self
    }
    /// `[optional account]`
    /// New lockup authority
    #[inline(always)]
    pub fn new_authority(
        &mut self,
        new_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    ) -> &mut Self {
        self.instruction.new_authority = new_authority;
        self
    }
    /// `[optional argument]`
    #[inline(always)]
    pub fn unix_timestamp(&mut self, unix_timestamp: i64) -> &mut Self {
        self.instruction.unix_timestamp = Some(unix_timestamp);
        self
    }
    /// `[optional argument]`
    #[inline(always)]
    pub fn epoch(&mut self, epoch: u64) -> &mut Self {
        self.instruction.epoch = Some(epoch);
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
        let args = SetLockupCheckedInstructionArgs {
            unix_timestamp: self.instruction.unix_timestamp.clone(),
            epoch: self.instruction.epoch.clone(),
        };
        let instruction = SetLockupCheckedCpi {
            __program: self.instruction.__program,

            stake: self.instruction.stake.expect("stake is not set"),

            authority: self.instruction.authority.expect("authority is not set"),

            new_authority: self.instruction.new_authority,
            __args: args,
        };
        instruction.invoke_signed_with_remaining_accounts(
            signers_seeds,
            &self.instruction.__remaining_accounts,
        )
    }
}

#[derive(Clone, Debug)]
struct SetLockupCheckedCpiBuilderInstruction<'a, 'b> {
    __program: &'b solana_program::account_info::AccountInfo<'a>,
    stake: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    new_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    unix_timestamp: Option<i64>,
    epoch: Option<u64>,
    /// Additional instruction accounts `(AccountInfo, is_writable, is_signer)`.
    __remaining_accounts: Vec<(
        &'b solana_program::account_info::AccountInfo<'a>,
        bool,
        bool,
    )>,
}
