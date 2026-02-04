use {
    super::{
        execution::ExecutionWithChecks,
        lifecycle::StakeLifecycle,
        utils::{add_sysvars, create_vote_account, STAKE_RENT_EXEMPTION},
    },
    mollusk_svm::{result::Check, Mollusk},
    solana_account::AccountSharedData,
    solana_instruction::Instruction,
    solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
    solana_stake_interface::state::Lockup,
    solana_stake_program::id,
};

/// Builder for creating stake accounts with customizable parameters
pub struct StakeAccountBuilder<'a> {
    ctx: &'a mut StakeTestContext,
    lifecycle: StakeLifecycle,
    staked_amount: u64,
    stake_authority: Option<Pubkey>,
    withdraw_authority: Option<Pubkey>,
    lockup: Option<Lockup>,
    vote_account: Option<Pubkey>,
    stake_pubkey: Option<Pubkey>,
}

impl StakeAccountBuilder<'_> {
    /// Set the staked amount (lamports delegated to validator)
    pub fn staked_amount(mut self, amount: u64) -> Self {
        self.staked_amount = amount;
        self
    }

    /// Set a custom stake authority (defaults to ctx.staker)
    pub fn stake_authority(mut self, authority: &Pubkey) -> Self {
        self.stake_authority = Some(*authority);
        self
    }

    /// Set a custom withdraw authority (defaults to ctx.withdrawer)
    pub fn withdraw_authority(mut self, authority: &Pubkey) -> Self {
        self.withdraw_authority = Some(*authority);
        self
    }

    /// Set a custom lockup (defaults to Lockup::default())
    pub fn lockup(mut self, lockup: &Lockup) -> Self {
        self.lockup = Some(*lockup);
        self
    }

    /// Set a custom vote account (defaults to ctx.vote_account)
    pub fn vote_account(mut self, vote_account: &Pubkey) -> Self {
        self.vote_account = Some(*vote_account);
        self
    }

    /// Set a specific stake account pubkey (defaults to Pubkey::new_unique())
    pub fn stake_pubkey(mut self, pubkey: &Pubkey) -> Self {
        self.stake_pubkey = Some(*pubkey);
        self
    }

    /// Build the stake account and return (pubkey, account_data)
    pub fn build(self) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = self.stake_pubkey.unwrap_or_else(Pubkey::new_unique);
        let vote_account_ref = self.vote_account.as_ref().unwrap_or_else(|| {
            self.ctx
                .vote_account
                .as_ref()
                .map(|(pk, _)| pk)
                .expect("vote_account required for this lifecycle")
        });
        let account = self.lifecycle.create_stake_account_fully_specified(
            &mut self.ctx.mollusk,
            &stake_pubkey,
            vote_account_ref,
            self.staked_amount,
            self.stake_authority.as_ref().unwrap_or(&self.ctx.staker),
            self.withdraw_authority
                .as_ref()
                .unwrap_or(&self.ctx.withdrawer),
            self.lockup.as_ref().unwrap_or(&Lockup::default()),
        );
        (stake_pubkey, account)
    }
}

pub struct StakeTestContext {
    pub mollusk: Mollusk,
    pub rent_exempt_reserve: u64,
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
    pub minimum_delegation: Option<u64>,
    /// Vote account (pubkey, account_data) for delegation tests
    pub vote_account: Option<(Pubkey, AccountSharedData)>,
}

impl StakeTestContext {
    pub fn new() -> Self {
        let mollusk = Mollusk::new(&id(), "solana_stake_program");
        Self {
            mollusk,
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
            minimum_delegation: None,
            vote_account: None,
        }
    }

    pub fn with_delegation() -> Self {
        let mollusk = Mollusk::new(&id(), "solana_stake_program");
        let minimum_delegation = solana_stake_program::get_minimum_delegation();
        Self {
            mollusk,
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
            minimum_delegation: Some(minimum_delegation),
            vote_account: Some((Pubkey::new_unique(), create_vote_account())),
        }
    }

    /// Create a stake account builder for the specified lifecycle stage
    ///
    /// Example:
    /// ```
    /// let (stake, account) = ctx
    ///     .stake_account(StakeLifecycle::Active)
    ///     .staked_amount(1_000_000)
    ///     .stake_account(StakeLifecycle::Active)
    ///     .staked_amount(1_000_000)
    ///     .build();
    /// ```
    pub fn stake_account(&mut self, lifecycle: StakeLifecycle) -> StakeAccountBuilder<'_> {
        StakeAccountBuilder {
            ctx: self,
            lifecycle,
            staked_amount: 0,
            stake_authority: None,
            withdraw_authority: None,
            lockup: None,
            vote_account: None,
            stake_pubkey: None,
        }
    }

    /// Configure execution with specific checks, then call .execute(instruction, accounts)
    ///
    /// Usage: `ctx.checks(&checks).execute(instruction, accounts)`
    pub fn checks<'a, 'b>(&'a mut self, checks: &'b [Check<'b>]) -> ExecutionWithChecks<'a, 'b> {
        ExecutionWithChecks::new(self, checks)
    }

    /// Create a lockup that expires in the future
    pub fn create_future_lockup(&self, epochs_ahead: u64) -> Lockup {
        Lockup {
            unix_timestamp: 0,
            epoch: self.mollusk.sysvars.clock.epoch + epochs_ahead,
            custodian: Pubkey::new_unique(),
        }
    }

    /// Create a lockup that's currently in force (far future)
    pub fn create_in_force_lockup(&self) -> Lockup {
        self.create_future_lockup(1_000_000)
    }

    /// Create a second vote account (for testing different vote accounts)
    pub fn create_second_vote_account(&self) -> (Pubkey, AccountSharedData) {
        (Pubkey::new_unique(), create_vote_account())
    }

    /// Execute an instruction with default success checks and missing signer testing
    ///
    /// Usage: `ctx.execute(instruction, accounts)`
    pub fn execute(
        &mut self,
        instruction: Instruction,
        accounts: &[(&Pubkey, &AccountSharedData)],
    ) -> mollusk_svm::result::InstructionResult {
        self.execute_internal(instruction, accounts, &[Check::success()], true)
    }

    /// Internal: execute with given checks and current config
    pub(crate) fn execute_internal(
        &mut self,
        instruction: Instruction,
        accounts: &[(&Pubkey, &AccountSharedData)],
        checks: &[Check],
        test_missing_signers: bool,
    ) -> mollusk_svm::result::InstructionResult {
        let accounts_vec: Vec<(Pubkey, AccountSharedData)> = accounts
            .iter()
            .map(|(pk, data)| (**pk, (*data).clone()))
            .collect();

        if test_missing_signers {
            verify_all_signers_required(&self.mollusk, &instruction, &accounts_vec);
        }

        // Process with all signers present
        let accounts_with_sysvars = add_sysvars(&self.mollusk, &instruction, accounts_vec);
        self.mollusk
            .process_and_validate_instruction(&instruction, &accounts_with_sysvars, checks)
    }
}

impl Default for StakeTestContext {
    fn default() -> Self {
        Self::with_delegation()
    }
}

/// Verify that removing any signer from the instruction causes MissingRequiredSignature error
fn verify_all_signers_required(
    mollusk: &Mollusk,
    instruction: &Instruction,
    accounts: &[(Pubkey, AccountSharedData)],
) {
    for i in 0..instruction.accounts.len() {
        if instruction.accounts[i].is_signer {
            let mut modified_instruction = instruction.clone();
            modified_instruction.accounts[i].is_signer = false;

            let accounts_with_sysvars =
                add_sysvars(mollusk, &modified_instruction, accounts.to_vec());

            mollusk.process_and_validate_instruction(
                &modified_instruction,
                &accounts_with_sysvars,
                &[Check::err(ProgramError::MissingRequiredSignature)],
            );
        }
    }
}
