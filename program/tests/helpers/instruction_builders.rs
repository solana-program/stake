use {
    super::context::StakeTestContext, mollusk_svm::result::Check,
    solana_account::AccountSharedData, solana_instruction::Instruction, solana_pubkey::Pubkey,
};

/// Execution builder with validation and signer testing
pub struct InstructionExecution<'a, 'b> {
    instruction: Instruction,
    accounts: Vec<(Pubkey, AccountSharedData)>,
    ctx: &'a StakeTestContext,
    checks: Option<&'b [Check<'b>]>,
    test_missing_signers: Option<bool>, // `None` runs if `Check::success`
}

impl<'b> InstructionExecution<'_, 'b> {
    pub fn checks(mut self, checks: &'b [Check<'b>]) -> Self {
        self.checks = Some(checks);
        self
    }

    pub fn test_missing_signers(mut self, test: bool) -> Self {
        self.test_missing_signers = Some(test);
        self
    }

    /// Executes the instruction. If `checks` is `None` or empty, uses `Check::success()`.
    /// Fail-safe default: when `test_missing_signers` is `None`, runs the missing-signers
    /// test (`true`). Callers must explicitly opt out with `.test_missing_signers(false)`.
    pub fn execute(self) -> mollusk_svm::result::InstructionResult {
        let default_checks = [Check::success()];
        let checks = match self.checks {
            Some(c) if !c.is_empty() => c,
            _ => &default_checks,
        };

        let test_missing_signers = self.test_missing_signers.unwrap_or(true);

        self.ctx.process_instruction_maybe_test_signers(
            &self.instruction,
            self.accounts,
            checks,
            test_missing_signers,
        )
    }
}

impl<'a> InstructionExecution<'a, '_> {
    pub(crate) fn new(
        instruction: Instruction,
        accounts: Vec<(Pubkey, AccountSharedData)>,
        ctx: &'a StakeTestContext,
    ) -> Self {
        Self {
            instruction,
            accounts,
            ctx,
            checks: None,
            test_missing_signers: None,
        }
    }
}
