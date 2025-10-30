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

    /// Create a stake account at the specified lifecycle stage with standard authorities
    pub fn create_stake_account(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
    ) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = Pubkey::new_unique();
        let account = lifecycle.create_stake_account_fully_specified(
            &mut self.mollusk,
            &mut self.tracker,
            &stake_pubkey,
            &self.vote_account,
            staked_amount,
            &self.staker,
            &self.withdrawer,
            &Lockup::default(),
        );
        (stake_pubkey, account)
    }

    /// Create a stake account with custom lockup
    pub fn create_stake_account_with_lockup(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
        lockup: &Lockup,
    ) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = Pubkey::new_unique();
        let account = lifecycle.create_stake_account_fully_specified(
            &mut self.mollusk,
            &mut self.tracker,
            &stake_pubkey,
            &self.vote_account,
            staked_amount,
            &self.staker,
            &self.withdrawer,
            lockup,
        );
        (stake_pubkey, account)
    }

    /// Create a stake account with custom authorities
    pub fn create_stake_account_with_authorities(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
        staker: &Pubkey,
        withdrawer: &Pubkey,
    ) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = Pubkey::new_unique();
        let account = lifecycle.create_stake_account_fully_specified(
            &mut self.mollusk,
            &mut self.tracker,
            &stake_pubkey,
            &self.vote_account,
            staked_amount,
            staker,
            withdrawer,
            &Lockup::default(),
        );
        (stake_pubkey, account)
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
