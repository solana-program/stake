use {
    super::context::StakeTestContext, mollusk_svm::result::Check,
    solana_account::AccountSharedData, solana_instruction::Instruction, solana_pubkey::Pubkey,
};

/// Wrapper for executing with specific checks
///
/// Usage: `ctx.checks(&checks).test_missing_signers(false).execute(instruction, accounts)`
pub struct ExecutionWithChecks<'a, 'b> {
    pub(crate) ctx: &'a mut StakeTestContext,
    pub(crate) checks: &'b [Check<'b>],
    pub(crate) test_missing_signers: bool,
}

impl<'a, 'b> ExecutionWithChecks<'a, 'b> {
    pub fn new(ctx: &'a mut StakeTestContext, checks: &'b [Check<'b>]) -> Self {
        Self {
            ctx,
            checks,
            test_missing_signers: true, // default: test missing signers
        }
    }

    pub fn test_missing_signers(mut self, test: bool) -> Self {
        self.test_missing_signers = test;
        self
    }

    pub fn execute(
        self,
        instruction: Instruction,
        accounts: &[(&Pubkey, &AccountSharedData)],
    ) -> mollusk_svm::result::InstructionResult {
        self.ctx.execute_internal(
            instruction,
            accounts,
            self.checks,
            self.test_missing_signers,
        )
    }
}
