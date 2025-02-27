//! This code was AUTOGENERATED using the codama library.
//! Please DO NOT EDIT THIS FILE, instead use visitors
//! to add features, then rerun codama to update it.
//!
//! <https://github.com/codama-idl/codama>
//!

use {
    crate::generated::types::StakeAuthorize,
    borsh::{BorshDeserialize, BorshSerialize},
    solana_program::pubkey::Pubkey,
};

/// Accounts.
pub struct AuthorizeWithSeed {
    /// Stake account to be updated
    pub stake: solana_program::pubkey::Pubkey,
    /// Base key of stake or withdraw authority
    pub base: solana_program::pubkey::Pubkey,
    /// Clock sysvar
    pub clock_sysvar: solana_program::pubkey::Pubkey,
    /// Lockup authority
    pub lockup_authority: Option<solana_program::pubkey::Pubkey>,
}

impl AuthorizeWithSeed {
    pub fn instruction(
        &self,
        args: AuthorizeWithSeedInstructionArgs,
    ) -> solana_program::instruction::Instruction {
        self.instruction_with_remaining_accounts(args, &[])
    }
    #[allow(clippy::vec_init_then_push)]
    pub fn instruction_with_remaining_accounts(
        &self,
        args: AuthorizeWithSeedInstructionArgs,
        remaining_accounts: &[solana_program::instruction::AccountMeta],
    ) -> solana_program::instruction::Instruction {
        let mut accounts = Vec::with_capacity(4 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            self.stake, false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.base, true,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            self.clock_sysvar,
            false,
        ));
        if let Some(lockup_authority) = self.lockup_authority {
            accounts.push(solana_program::instruction::AccountMeta::new_readonly(
                lockup_authority,
                true,
            ));
        } else {
            accounts.push(solana_program::instruction::AccountMeta::new_readonly(
                crate::STAKE_ID,
                false,
            ));
        }
        accounts.extend_from_slice(remaining_accounts);
        let mut data = AuthorizeWithSeedInstructionData::new()
            .try_to_vec()
            .unwrap();
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
pub struct AuthorizeWithSeedInstructionData {
    discriminator: u32,
}

impl AuthorizeWithSeedInstructionData {
    pub fn new() -> Self {
        Self { discriminator: 8 }
    }
}

impl Default for AuthorizeWithSeedInstructionData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AuthorizeWithSeedInstructionArgs {
    pub new_authorized_pubkey: Pubkey,
    pub stake_authorize: StakeAuthorize,
    pub authority_seed: String,
    pub authority_owner: Pubkey,
}

/// Instruction builder for `AuthorizeWithSeed`.
///
/// ### Accounts:
///
///   0. `[writable]` stake
///   1. `[signer]` base
///   2. `[optional]` clock_sysvar (default to `SysvarC1ock11111111111111111111111111111111`)
///   3. `[signer, optional]` lockup_authority
#[derive(Clone, Debug, Default)]
pub struct AuthorizeWithSeedBuilder {
    stake: Option<solana_program::pubkey::Pubkey>,
    base: Option<solana_program::pubkey::Pubkey>,
    clock_sysvar: Option<solana_program::pubkey::Pubkey>,
    lockup_authority: Option<solana_program::pubkey::Pubkey>,
    new_authorized_pubkey: Option<Pubkey>,
    stake_authorize: Option<StakeAuthorize>,
    authority_seed: Option<String>,
    authority_owner: Option<Pubkey>,
    __remaining_accounts: Vec<solana_program::instruction::AccountMeta>,
}

impl AuthorizeWithSeedBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    /// Stake account to be updated
    #[inline(always)]
    pub fn stake(&mut self, stake: solana_program::pubkey::Pubkey) -> &mut Self {
        self.stake = Some(stake);
        self
    }
    /// Base key of stake or withdraw authority
    #[inline(always)]
    pub fn base(&mut self, base: solana_program::pubkey::Pubkey) -> &mut Self {
        self.base = Some(base);
        self
    }
    /// `[optional account, default to 'SysvarC1ock11111111111111111111111111111111']`
    /// Clock sysvar
    #[inline(always)]
    pub fn clock_sysvar(&mut self, clock_sysvar: solana_program::pubkey::Pubkey) -> &mut Self {
        self.clock_sysvar = Some(clock_sysvar);
        self
    }
    /// `[optional account]`
    /// Lockup authority
    #[inline(always)]
    pub fn lockup_authority(
        &mut self,
        lockup_authority: Option<solana_program::pubkey::Pubkey>,
    ) -> &mut Self {
        self.lockup_authority = lockup_authority;
        self
    }
    #[inline(always)]
    pub fn new_authorized_pubkey(&mut self, new_authorized_pubkey: Pubkey) -> &mut Self {
        self.new_authorized_pubkey = Some(new_authorized_pubkey);
        self
    }
    #[inline(always)]
    pub fn stake_authorize(&mut self, stake_authorize: StakeAuthorize) -> &mut Self {
        self.stake_authorize = Some(stake_authorize);
        self
    }
    #[inline(always)]
    pub fn authority_seed(&mut self, authority_seed: String) -> &mut Self {
        self.authority_seed = Some(authority_seed);
        self
    }
    #[inline(always)]
    pub fn authority_owner(&mut self, authority_owner: Pubkey) -> &mut Self {
        self.authority_owner = Some(authority_owner);
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
        let accounts = AuthorizeWithSeed {
            stake: self.stake.expect("stake is not set"),
            base: self.base.expect("base is not set"),
            clock_sysvar: self.clock_sysvar.unwrap_or(solana_program::pubkey!(
                "SysvarC1ock11111111111111111111111111111111"
            )),
            lockup_authority: self.lockup_authority,
        };
        let args = AuthorizeWithSeedInstructionArgs {
            new_authorized_pubkey: self
                .new_authorized_pubkey
                .clone()
                .expect("new_authorized_pubkey is not set"),
            stake_authorize: self
                .stake_authorize
                .clone()
                .expect("stake_authorize is not set"),
            authority_seed: self
                .authority_seed
                .clone()
                .expect("authority_seed is not set"),
            authority_owner: self
                .authority_owner
                .clone()
                .expect("authority_owner is not set"),
        };

        accounts.instruction_with_remaining_accounts(args, &self.__remaining_accounts)
    }
}

/// `authorize_with_seed` CPI accounts.
pub struct AuthorizeWithSeedCpiAccounts<'a, 'b> {
    /// Stake account to be updated
    pub stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Base key of stake or withdraw authority
    pub base: &'b solana_program::account_info::AccountInfo<'a>,
    /// Clock sysvar
    pub clock_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
    /// Lockup authority
    pub lockup_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
}

/// `authorize_with_seed` CPI instruction.
pub struct AuthorizeWithSeedCpi<'a, 'b> {
    /// The program to invoke.
    pub __program: &'b solana_program::account_info::AccountInfo<'a>,
    /// Stake account to be updated
    pub stake: &'b solana_program::account_info::AccountInfo<'a>,
    /// Base key of stake or withdraw authority
    pub base: &'b solana_program::account_info::AccountInfo<'a>,
    /// Clock sysvar
    pub clock_sysvar: &'b solana_program::account_info::AccountInfo<'a>,
    /// Lockup authority
    pub lockup_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    /// The arguments for the instruction.
    pub __args: AuthorizeWithSeedInstructionArgs,
}

impl<'a, 'b> AuthorizeWithSeedCpi<'a, 'b> {
    pub fn new(
        program: &'b solana_program::account_info::AccountInfo<'a>,
        accounts: AuthorizeWithSeedCpiAccounts<'a, 'b>,
        args: AuthorizeWithSeedInstructionArgs,
    ) -> Self {
        Self {
            __program: program,
            stake: accounts.stake,
            base: accounts.base,
            clock_sysvar: accounts.clock_sysvar,
            lockup_authority: accounts.lockup_authority,
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
        let mut accounts = Vec::with_capacity(4 + remaining_accounts.len());
        accounts.push(solana_program::instruction::AccountMeta::new(
            *self.stake.key,
            false,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.base.key,
            true,
        ));
        accounts.push(solana_program::instruction::AccountMeta::new_readonly(
            *self.clock_sysvar.key,
            false,
        ));
        if let Some(lockup_authority) = self.lockup_authority {
            accounts.push(solana_program::instruction::AccountMeta::new_readonly(
                *lockup_authority.key,
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
        let mut data = AuthorizeWithSeedInstructionData::new()
            .try_to_vec()
            .unwrap();
        let mut args = self.__args.try_to_vec().unwrap();
        data.append(&mut args);

        let instruction = solana_program::instruction::Instruction {
            program_id: crate::STAKE_ID,
            accounts,
            data,
        };
        let mut account_infos = Vec::with_capacity(5 + remaining_accounts.len());
        account_infos.push(self.__program.clone());
        account_infos.push(self.stake.clone());
        account_infos.push(self.base.clone());
        account_infos.push(self.clock_sysvar.clone());
        if let Some(lockup_authority) = self.lockup_authority {
            account_infos.push(lockup_authority.clone());
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

/// Instruction builder for `AuthorizeWithSeed` via CPI.
///
/// ### Accounts:
///
///   0. `[writable]` stake
///   1. `[signer]` base
///   2. `[]` clock_sysvar
///   3. `[signer, optional]` lockup_authority
#[derive(Clone, Debug)]
pub struct AuthorizeWithSeedCpiBuilder<'a, 'b> {
    instruction: Box<AuthorizeWithSeedCpiBuilderInstruction<'a, 'b>>,
}

impl<'a, 'b> AuthorizeWithSeedCpiBuilder<'a, 'b> {
    pub fn new(program: &'b solana_program::account_info::AccountInfo<'a>) -> Self {
        let instruction = Box::new(AuthorizeWithSeedCpiBuilderInstruction {
            __program: program,
            stake: None,
            base: None,
            clock_sysvar: None,
            lockup_authority: None,
            new_authorized_pubkey: None,
            stake_authorize: None,
            authority_seed: None,
            authority_owner: None,
            __remaining_accounts: Vec::new(),
        });
        Self { instruction }
    }
    /// Stake account to be updated
    #[inline(always)]
    pub fn stake(&mut self, stake: &'b solana_program::account_info::AccountInfo<'a>) -> &mut Self {
        self.instruction.stake = Some(stake);
        self
    }
    /// Base key of stake or withdraw authority
    #[inline(always)]
    pub fn base(&mut self, base: &'b solana_program::account_info::AccountInfo<'a>) -> &mut Self {
        self.instruction.base = Some(base);
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
    /// `[optional account]`
    /// Lockup authority
    #[inline(always)]
    pub fn lockup_authority(
        &mut self,
        lockup_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    ) -> &mut Self {
        self.instruction.lockup_authority = lockup_authority;
        self
    }
    #[inline(always)]
    pub fn new_authorized_pubkey(&mut self, new_authorized_pubkey: Pubkey) -> &mut Self {
        self.instruction.new_authorized_pubkey = Some(new_authorized_pubkey);
        self
    }
    #[inline(always)]
    pub fn stake_authorize(&mut self, stake_authorize: StakeAuthorize) -> &mut Self {
        self.instruction.stake_authorize = Some(stake_authorize);
        self
    }
    #[inline(always)]
    pub fn authority_seed(&mut self, authority_seed: String) -> &mut Self {
        self.instruction.authority_seed = Some(authority_seed);
        self
    }
    #[inline(always)]
    pub fn authority_owner(&mut self, authority_owner: Pubkey) -> &mut Self {
        self.instruction.authority_owner = Some(authority_owner);
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
        let args = AuthorizeWithSeedInstructionArgs {
            new_authorized_pubkey: self
                .instruction
                .new_authorized_pubkey
                .clone()
                .expect("new_authorized_pubkey is not set"),
            stake_authorize: self
                .instruction
                .stake_authorize
                .clone()
                .expect("stake_authorize is not set"),
            authority_seed: self
                .instruction
                .authority_seed
                .clone()
                .expect("authority_seed is not set"),
            authority_owner: self
                .instruction
                .authority_owner
                .clone()
                .expect("authority_owner is not set"),
        };
        let instruction = AuthorizeWithSeedCpi {
            __program: self.instruction.__program,

            stake: self.instruction.stake.expect("stake is not set"),

            base: self.instruction.base.expect("base is not set"),

            clock_sysvar: self
                .instruction
                .clock_sysvar
                .expect("clock_sysvar is not set"),

            lockup_authority: self.instruction.lockup_authority,
            __args: args,
        };
        instruction.invoke_signed_with_remaining_accounts(
            signers_seeds,
            &self.instruction.__remaining_accounts,
        )
    }
}

#[derive(Clone, Debug)]
struct AuthorizeWithSeedCpiBuilderInstruction<'a, 'b> {
    __program: &'b solana_program::account_info::AccountInfo<'a>,
    stake: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    base: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    clock_sysvar: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    lockup_authority: Option<&'b solana_program::account_info::AccountInfo<'a>>,
    new_authorized_pubkey: Option<Pubkey>,
    stake_authorize: Option<StakeAuthorize>,
    authority_seed: Option<String>,
    authority_owner: Option<Pubkey>,
    /// Additional instruction accounts `(AccountInfo, is_writable, is_signer)`.
    __remaining_accounts: Vec<(
        &'b solana_program::account_info::AccountInfo<'a>,
        bool,
        bool,
    )>,
}
