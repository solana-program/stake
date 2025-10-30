use {
    super::{
        instruction_builders::{InstructionConfig, InstructionExecution},
        lifecycle::StakeLifecycle,
        stake_tracker::StakeTracker,
        utils::{add_sysvars, create_vote_account, STAKE_RENT_EXEMPTION},
    },
    mollusk_svm::{result::Check, Mollusk},
    solana_account::AccountSharedData,
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_stake_interface::state::Lockup,
    solana_stake_program::id,
};

/// Builder for creating stake accounts with customizable parameters
/// Follows the builder pattern for flexibility and readability
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

impl<'a> StakeAccountBuilder<'a> {
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
        let account = self.lifecycle.create_stake_account_fully_specified(
            &mut self.ctx.mollusk,
            &mut self.ctx.tracker,
            &stake_pubkey,
            self.vote_account.as_ref().unwrap_or(&self.ctx.vote_account),
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

/// Consolidated test context that bundles all common test setup
/// This eliminates 8-10 lines of boilerplate from every test
pub struct StakeTestContext {
    pub mollusk: Mollusk,
    pub tracker: StakeTracker,
    pub minimum_delegation: u64,
    pub rent_exempt_reserve: u64,
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
    pub vote_account: Pubkey,
    pub vote_account_data: AccountSharedData,
}

impl StakeTestContext {
    /// Create a new test context with all standard setup
    pub fn new() -> Self {
        let mollusk = Mollusk::new(&id(), "solana_stake_program");
        let minimum_delegation = solana_stake_program::get_minimum_delegation();
        let tracker = StakeLifecycle::create_tracker_for_test(minimum_delegation);

        Self {
            mollusk,
            tracker,
            minimum_delegation,
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
            vote_account: Pubkey::new_unique(),
            vote_account_data: create_vote_account(),
        }
    }

    /// Create a stake account builder for the specified lifecycle stage
    /// This is the primary method for creating stake accounts in tests.
    ///
    /// Example:
    /// ```
    /// let (stake, account) = ctx
    ///     .stake_account(StakeLifecycle::Active)
    ///     .staked_amount(1_000_000)
    ///     .build();
    /// ```
    pub fn stake_account(&mut self, lifecycle: StakeLifecycle) -> StakeAccountBuilder {
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

    /// Create a stake account at the specified lifecycle stage with standard authorities
    /// DEPRECATED: Use `stake_account()` builder instead
    pub fn create_stake_account(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
    ) -> (Pubkey, AccountSharedData) {
        self.stake_account(lifecycle)
            .staked_amount(staked_amount)
            .build()
    }

    /// Create a stake account with custom lockup
    /// DEPRECATED: Use `stake_account()` builder instead
    pub fn create_stake_account_with_lockup(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
        lockup: &Lockup,
    ) -> (Pubkey, AccountSharedData) {
        self.stake_account(lifecycle)
            .staked_amount(staked_amount)
            .lockup(lockup)
            .build()
    }

    /// Create a stake account with custom authorities
    /// DEPRECATED: Use `stake_account()` builder instead
    pub fn create_stake_account_with_authorities(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
        staker: &Pubkey,
        withdrawer: &Pubkey,
    ) -> (Pubkey, AccountSharedData) {
        self.stake_account(lifecycle)
            .staked_amount(staked_amount)
            .stake_authority(staker)
            .withdraw_authority(withdrawer)
            .build()
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

    /// Process an instruction with a config-based approach
    pub fn process_with<'b, C: InstructionConfig>(
        &self,
        config: C,
    ) -> InstructionExecution<'_, 'b> {
        InstructionExecution::new(
            config.build_instruction(self),
            config.build_accounts(),
            self,
        )
    }

    /// Internal helper to process an instruction with optional missing signer testing
    pub(crate) fn process_instruction_maybe_test_signers(
        &self,
        instruction: &Instruction,
        accounts: Vec<(Pubkey, AccountSharedData)>,
        checks: &[Check],
        test_missing_signers: bool,
    ) -> mollusk_svm::result::InstructionResult {
        if test_missing_signers {
            use solana_program_error::ProgramError;

            // Test that removing each signer causes failure
            for i in 0..instruction.accounts.len() {
                if instruction.accounts[i].is_signer {
                    let mut modified_instruction = instruction.clone();
                    modified_instruction.accounts[i].is_signer = false;

                    let accounts_with_sysvars =
                        add_sysvars(&self.mollusk, &modified_instruction, accounts.clone());

                    self.mollusk.process_and_validate_instruction(
                        &modified_instruction,
                        &accounts_with_sysvars,
                        &[Check::err(ProgramError::MissingRequiredSignature)],
                    );
                }
            }
        }

        // Process with all signers present
        let accounts_with_sysvars = add_sysvars(&self.mollusk, instruction, accounts);
        self.mollusk
            .process_and_validate_instruction(instruction, &accounts_with_sysvars, checks)
    }
}

impl Default for StakeTestContext {
    fn default() -> Self {
        Self::new()
    }
}
