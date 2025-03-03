//! This code was AUTOGENERATED using the codama library.
//! Please DO NOT EDIT THIS FILE, instead use visitors
//! to add features, then rerun codama to update it.
//!
//! <https://github.com/codama-idl/codama>
//!

use borsh::{BorshDeserialize, BorshSerialize};

/// Accounts.
#[derive(Debug)]
pub struct Merge {
    /// Destination stake account
    pub destination_stake: solana_program::pubkey::Pubkey,
    /// Source stake account
    pub source_stake: solana_program::pubkey::Pubkey,
    /// Clock sysvar
    pub clock_sysvar: solana_program::pubkey::Pubkey,
    /// Stake history sysvar
    pub stake_history: solana_program::pubkey::Pubkey,
    /// Stake authority
    pub stake_authority: solana_program::pubkey::Pubkey,
}

impl Merge {
    pub fn instruction(&self) -> solana_program::instruction::Instruction {
        self.instruction_with_remaining_accounts(&[])
    }
    #[allow(clippy::vec_init_then_push)]
    pub fn instruction_with_remaining_accounts(
        &self,
        remaining_accounts: &[solana_program::instruction::AccountMeta],
    ) -> solana_program::instruction::Instruction {
        let mut accounts = Vec::with_capacity(5 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.destination_stake,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.source_stake,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.clock_sysvar,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.stake_history,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.stake_authority,
            true,
        ));
        accounts.extend_from_slice(remaining_accounts);
        let data = borsh::to_vec(&MergeInstructionData::new()).unwrap();

        solana_program::instruction::Instruction {
            program_id: crate::STAKE_ID,
            accounts,
            data,
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MergeInstructionData {
    discriminator: u8,
}

impl MergeInstructionData {
    pub fn new() -> Self {
        Self { discriminator: 7 }
    }
}

impl Default for MergeInstructionData {
    fn default() -> Self {
        Self::new()
    }
}

/// Instruction builder for `Merge`.
///
/// ### Accounts:
///
///   0. `[writable]` destination_stake
///   1. `[writable]` source_stake
///   2. `[optional]` clock_sysvar (default to `SysvarC1ock11111111111111111111111111111111`)
///   3. `[]` stake_history
///   4. `[signer]` stake_authority
#[derive(Clone, Debug, Default)]
pub struct MergeBuilder {
    destination_stake: Option<solana_program::pubkey::Pubkey>,
    source_stake: Option<solana_program::pubkey::Pubkey>,
    clock_sysvar: Option<solana_program::pubkey::Pubkey>,
    stake_history: Option<solana_program::pubkey::Pubkey>,
    stake_authority: Option<solana_program::pubkey::Pubkey>,
    __remaining_accounts: Vec<solana_program::instruction::AccountMeta>,
}

impl MergeBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    /// Destination stake account
    #[inline(always)]
    pub fn destination_stake(
        &mut self,
        destination_stake: solana_program::pubkey::Pubkey,
    ) -> &mut Self {
        self.destination_stake = Some(destination_stake);
        self
    }
    /// Source stake account
    #[inline(always)]
    pub fn source_stake(&mut self, source_stake: solana_program::pubkey::Pubkey) -> &mut Self {
        self.source_stake = Some(source_stake);
        self
    }
    /// `[optional account, default to 'SysvarC1ock11111111111111111111111111111111']`
    /// Clock sysvar
    #[inline(always)]
    pub fn clock_sysvar(&mut self, clock_sysvar: solana_program::pubkey::Pubkey) -> &mut Self {
        self.clock_sysvar = Some(clock_sysvar);
        self
    }
    /// Stake history sysvar
    #[inline(always)]
    pub fn stake_history(&mut self, stake_history: solana_program::pubkey::Pubkey) -> &mut Self {
        self.stake_history = Some(stake_history);
        self
    }
    /// Stake authority
    #[inline(always)]
    pub fn stake_authority(
        &mut self,
        stake_authority: solana_program::pubkey::Pubkey,
    ) -> &mut Self {
        self.stake_authority = Some(stake_authority);
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
        let accounts = Merge {
            destination_stake: self
                .destination_stake
                .expect("destination_stake is not set"),
            source_stake: self.source_stake.expect("source_stake is not set"),
            clock_sysvar: self.clock_sysvar.unwrap_or(solana_program::pubkey!(
                "SysvarC1ock11111111111111111111111111111111"
            )),
            stake_history: self.stake_history.expect("stake_history is not set"),
            stake_authority: self.stake_authority.expect("stake_authority is not set"),
        };

        accounts.instruction_with_remaining_accounts(&self.__remaining_accounts)
    }
}

/// `merge` CPI accounts.
pub struct MergeCpiAccounts<'a, 'b> {
    /// Destination stake account
    pub destination_stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Source stake account
    pub source_stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Clock sysvar
    pub clock_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
    /// Stake history sysvar
    pub stake_history: &'b solana_program::account_info::AccountInfo<'a>,
    /// Stake authority
    pub stake_authority: &'b solana_program::account_info::AccountInfo<'a>,
}

/// `merge` CPI instruction.
pub struct MergeCpi<'a, 'b> {
    /// The program to invoke.
    pub __program: &'b solana_program::account_info::AccountInfo<'a>,
    /// Destination stake account
    pub destination_stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Source stake account
    pub source_stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Clock sysvar
    pub clock_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
    /// Stake history sysvar
    pub stake_history: &'b solana_program::account_info::AccountInfo<'a>,
    /// Stake authority
    pub stake_authority: &'b solana_program::account_info::AccountInfo<'a>,
}

impl<'a, 'b> MergeCpi<'a, 'b> {
    pub fn new(
        program: &'b solana_program::account_info::AccountInfo<'a>,
        accounts: MergeCpiAccounts<'a, 'b>,
    ) -> Self {
        Self {
            __program: program,
            destination_stake: accounts.destination_stake,
            source_stake: accounts.source_stake,
            clock_sysvar: accounts.clock_sysvar,
            stake_history: accounts.stake_history,
            stake_authority: accounts.stake_authority,
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
        let mut accounts = Vec::with_capacity(5 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.destination_stake.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.source_stake.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.clock_sysvar.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.stake_history.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.stake_authority.key,
            true,
        ));
        remaining_accounts.iter().for_each(|remaining_account| {
            accounts.push(solana_program::instruction::AccountMeta {
                pubkey: *remaining_account.0.key,
                is_signer: remaining_account.1,
                is_writable: remaining_account.2,
            })
        });
        let data = borsh::to_vec(&MergeInstructionData::new()).unwrap();

        let instruction = solana_program::instruction::Instruction {
            program_id: crate::STAKE_ID,
            accounts,
            data,
        };
        let mut account_infos = Vec::with_capacity(6 + remaining_accounts.len());
        account_infos.push(self.__program.clone());
        account_infos.push(self.destination_stake.clone());
        account_infos.push(self.source_stake.clone());
        account_infos.push(self.clock_sysvar.clone());
        account_infos.push(self.stake_history.clone());
        account_infos.push(self.stake_authority.clone());
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

/// Instruction builder for `Merge` via CPI.
///
/// ### Accounts:
///
///   0. `[writable]` destination_stake
///   1. `[writable]` source_stake
///   2. `[]` clock_sysvar
///   3. `[]` stake_history
///   4. `[signer]` stake_authority
#[derive(Clone, Debug)]
pub struct MergeCpiBuilder<'a, 'b> {
    instruction: Box<MergeCpiBuilderInstruction<'a, 'b>>,
}

impl<'a, 'b> MergeCpiBuilder<'a, 'b> {
    pub fn new(program: &'b solana_program::account_info::AccountInfo<'a>) -> Self {
        let instruction = Box::new(MergeCpiBuilderInstruction {
            __program: program,
            destination_stake: None,
            source_stake: None,
            clock_sysvar: None,
            stake_history: None,
            stake_authority: None,
            __remaining_accounts: Vec::new(),
        });
        Self { instruction }
    }
    /// Destination stake account
    #[inline(always)]
    pub fn destination_stake(
        &mut self,
        destination_stake: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.destination_stake = Some(destination_stake);
        self
    }
    /// Source stake account
    #[inline(always)]
    pub fn source_stake(
        &mut self,
        source_stake: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.source_stake = Some(source_stake);
        self
    }
    /// Clock sysvar
    #[inline(always)]
    pub fn clock_sysvar(
        &mut self,
        clock_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.clock_sysvar = Some(clock_sysvar);
        self
    }
    /// Stake history sysvar
    #[inline(always)]
    pub fn stake_history(
        &mut self,
        stake_history: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.stake_history = Some(stake_history);
        self
    }
    /// Stake authority
    #[inline(always)]
    pub fn stake_authority(
        &mut self,
        stake_authority: &'b solana_program::account_info::AccountInfo<'a>,
    ) -> &mut Self {
        self.instruction.stake_authority = Some(stake_authority);
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
        let instruction = MergeCpi {
            __program: self.instruction.__program,

            destination_stake: self
                .instruction
                .destination_stake
                .expect("destination_stake is not set"),

            source_stake: self
                .instruction
                .source_stake
                .expect("source_stake is not set"),

            clock_sysvar: self
                .instruction
                .clock_sysvar
                .expect("clock_sysvar is not set"),

            stake_history: self
                .instruction
                .stake_history
                .expect("stake_history is not set"),

            stake_authority: self
                .instruction
                .stake_authority
                .expect("stake_authority is not set"),
        };
        instruction.invoke_signed_with_remaining_accounts(
            signers_seeds,
            &self.instruction.__remaining_accounts,
        )
    }
}

#[derive(Clone, Debug)]
struct MergeCpiBuilderInstruction<'a, 'b> {
    __program: &'b solana_program::account_info::AccountInfo<'a>,
    destination_stake: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    source_stake: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    clock_sysvar: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    stake_history: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    stake_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    /// Additional instruction accounts `(AccountInfo, is_writable, is_signer)`.
    __remaining_accounts: Vec<(
        &'b solana_program::account_info::AccountInfo<'a>,
        bool,
        bool,
    )>,
}
