use {
    super::{
        instruction_builders::InstructionExecution,
        lifecycle::StakeLifecycle,
        utils::{add_sysvars, STAKE_RENT_EXEMPTION},
    },
    mollusk_svm::{result::Check, Mollusk},
    solana_account::AccountSharedData,
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_stake_program::id,
};

/// Builder for creating stake accounts with customizable parameters
pub struct StakeAccountBuilder {
    lifecycle: StakeLifecycle,
}

impl StakeAccountBuilder {
    pub fn build(self) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = Pubkey::new_unique();
        let account = self.lifecycle.create_uninitialized_account();
        (stake_pubkey, account)
    }
}

/// Consolidated test context for stake account tests
pub struct StakeTestContext {
    pub mollusk: Mollusk,
    pub rent_exempt_reserve: u64,
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
}

impl StakeTestContext {
    /// Create a new test context with all standard setup
    pub fn new() -> Self {
        let mollusk = Mollusk::new(&id(), "solana_stake_program");

        Self {
            mollusk,
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
        }
    }

    /// Create a stake account builder for the specified lifecycle stage
    ///
    /// Example:
    /// ```
    /// let (stake, account) = ctx
    ///     .stake_account(StakeLifecycle::Uninitialized)
    ///     .build();
    /// ```
    pub fn stake_account(&mut self, lifecycle: StakeLifecycle) -> StakeAccountBuilder {
        StakeAccountBuilder { lifecycle }
    }

    /// Process an instruction with account data provided as a slice of (pubkey, data) pairs.
    /// Sysvars are auto-resolved - only provide data for accounts that need it.
    pub fn process<'b>(
        &self,
        instruction: Instruction,
        accounts: &[(&Pubkey, &AccountSharedData)],
    ) -> InstructionExecution<'_, 'b> {
        let accounts_vec = accounts
            .iter()
            .map(|(pk, data)| (**pk, (*data).clone()))
            .collect();
        InstructionExecution::new(instruction, accounts_vec, self)
    }

    /// Process an instruction with optional missing signer testing
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
