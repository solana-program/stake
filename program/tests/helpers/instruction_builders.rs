use {
    super::context::StakeTestContext, mollusk_svm::result::Check,
    solana_account::AccountSharedData, solana_instruction::Instruction, solana_pubkey::Pubkey,
    std::collections::HashMap,
};

/// Execution builder with account data, validation, and signer testing
pub struct InstructionExecution<'a, 'b> {
    instruction: Instruction,
    accounts: HashMap<Pubkey, AccountSharedData>,
    ctx: &'a StakeTestContext,
    checks: Option<&'b [Check<'b>]>,
    test_missing_signers: Option<bool>, // `None` runs if `Check::success`
}

impl<'a, 'b> InstructionExecution<'a, 'b> {
    pub(crate) fn new(instruction: Instruction, ctx: &'a StakeTestContext) -> Self {
        Self {
            instruction,
            accounts: HashMap::new(),
            ctx,
            checks: None,
            test_missing_signers: None,
        }
    }

    /// Add account data for a specific pubkey
    #[inline(always)]
    pub fn account(mut self, pubkey: Pubkey, data: AccountSharedData) -> Self {
        self.accounts.insert(pubkey, data);
        self
    }

    /// Add multiple accounts at once from an iterator
    #[allow(dead_code)]
    #[inline(always)]
    pub fn accounts(
        mut self,
        accounts: impl IntoIterator<Item = (Pubkey, AccountSharedData)>,
    ) -> Self {
        self.accounts.extend(accounts);
        self
    }

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

        // Build account list from instruction account metas
        let accounts_with_data: Vec<(Pubkey, AccountSharedData)> = self
            .instruction
            .accounts
            .iter()
            .filter_map(|meta| {
                self.accounts
                    .get(&meta.pubkey)
                    .map(|data| (meta.pubkey, data.clone()))
            })
            .collect();

        self.ctx.process_instruction_maybe_test_signers(
            &self.instruction,
            accounts_with_data,
            checks,
            test_missing_signers,
        )
    }
}
