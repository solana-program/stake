use {
    super::context::StakeTestContext,
    mollusk_svm::result::Check,
    solana_account::AccountSharedData,
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup},
    },
};

// Trait for instruction configuration that builds instruction and accounts
pub trait InstructionConfig {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction;
    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)>;
}

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

pub struct InitializeConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub authorized: &'a Authorized,
    pub lockup: &'a Lockup,
}

impl InstructionConfig for InitializeConfig<'_> {
    fn build_instruction(&self, _ctx: &StakeTestContext) -> Instruction {
        ixn::initialize(self.stake.0, self.authorized, self.lockup)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![(*self.stake.0, self.stake.1.clone())]
    }
}

pub struct InitializeCheckedConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub authorized: &'a Authorized,
}

impl InstructionConfig for InitializeCheckedConfig<'_> {
    fn build_instruction(&self, _ctx: &StakeTestContext) -> Instruction {
        ixn::initialize_checked(self.stake.0, self.authorized)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![(*self.stake.0, self.stake.1.clone())]
    }
}

pub struct DeactivateConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    /// Override signer for testing wrong signer scenarios (defaults to ctx.staker)
    pub override_signer: Option<&'a Pubkey>,
}

impl InstructionConfig for DeactivateConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let signer = self.override_signer.unwrap_or(&ctx.staker);
        ixn::deactivate_stake(self.stake.0, signer)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![(*self.stake.0, self.stake.1.clone())]
    }
}

pub struct DelegateConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub vote: (&'a Pubkey, &'a AccountSharedData),
}

impl InstructionConfig for DelegateConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        ixn::delegate_stake(self.stake.0, &ctx.staker, self.vote.0)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![
            (*self.stake.0, self.stake.1.clone()),
            (*self.vote.0, self.vote.1.clone()),
        ]
    }
}

pub struct DeactivateDelinquentConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub vote: (&'a Pubkey, &'a AccountSharedData),
    pub reference_vote: (&'a Pubkey, &'a AccountSharedData),
}

impl InstructionConfig for DeactivateDelinquentConfig<'_> {
    fn build_instruction(&self, _ctx: &StakeTestContext) -> Instruction {
        ixn::deactivate_delinquent_stake(self.stake.0, self.vote.0, self.reference_vote.0)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![
            (*self.stake.0, self.stake.1.clone()),
            (*self.vote.0, self.vote.1.clone()),
            (*self.reference_vote.0, self.reference_vote.1.clone()),
        ]
    }
}

pub struct WithdrawConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub recipient: (&'a Pubkey, &'a AccountSharedData),
    pub amount: u64,
    /// Override signer for testing wrong signer scenarios (defaults to ctx.withdrawer)
    pub override_signer: Option<&'a Pubkey>,
}

impl InstructionConfig for WithdrawConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let signer = self.override_signer.unwrap_or(&ctx.withdrawer);
        ixn::withdraw(self.stake.0, signer, self.recipient.0, self.amount, None)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![
            (*self.stake.0, self.stake.1.clone()),
            (*self.recipient.0, self.recipient.1.clone()),
        ]
    }
}
