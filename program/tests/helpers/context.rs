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

#[allow(dead_code)] // can be removed once later tests are in
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
        let account = self.lifecycle.create_stake_account_fully_specified(
            &mut self.ctx.mollusk,
            self.ctx
                .tracker
                .as_mut()
                .expect("tracker required for stake account builder"),
            &stake_pubkey,
            self.vote_account.as_ref().unwrap_or(
                self.ctx
                    .vote_account
                    .as_ref()
                    .expect("vote_account required for this lifecycle"),
            ),
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

#[allow(dead_code)] // can be removed once later tests are in
pub struct StakeTestContext {
    pub mollusk: Mollusk,
    pub rent_exempt_reserve: u64,
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
    pub minimum_delegation: Option<u64>,
    pub vote_account: Option<Pubkey>,
    pub vote_account_data: Option<AccountSharedData>,
    pub tracker: Option<StakeTracker>,
}

#[allow(dead_code)] // can be removed once later tests are in
impl StakeTestContext {
    pub fn minimal() -> Self {
        let mollusk = Mollusk::new(&id(), "solana_stake_program");
        Self {
            mollusk,
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
            minimum_delegation: None,
            vote_account: None,
            vote_account_data: None,
            tracker: None,
        }
    }

    pub fn with_delegation() -> Self {
        let mollusk = Mollusk::new(&id(), "solana_stake_program");
        let minimum_delegation = solana_stake_program::get_minimum_delegation();
        let tracker: StakeTracker = StakeLifecycle::create_tracker_for_test(minimum_delegation);
        Self {
            mollusk,
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
            minimum_delegation: Some(minimum_delegation),
            vote_account: Some(Pubkey::new_unique()),
            vote_account_data: Some(create_vote_account()),
            tracker: Some(tracker),
        }
    }

    pub fn new() -> Self {
        Self::with_delegation()
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
