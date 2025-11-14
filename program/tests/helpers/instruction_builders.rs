use {
    super::context::StakeTestContext,
    mollusk_svm::result::Check,
    solana_account::AccountSharedData,
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup, StakeAuthorize},
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

    /// Executes the instruction. If `checks` is `None`, uses `Check::success()`.
    /// If `checks` is `Some(&[])` (empty), runs without validation.
    /// When `test_missing_signers` is `None`, runs the missing-signers tests.
    /// Callers must explicitly opt out with `.test_missing_signers(false)`.
    pub fn execute(self) -> mollusk_svm::result::InstructionResult {
        let default_checks = [Check::success()];
        let checks = match self.checks {
            None => &default_checks,
            Some(c) => c,
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

pub struct AuthorizeConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub override_authority: Option<&'a Pubkey>,
    pub new_authority: &'a Pubkey,
    pub stake_authorize: StakeAuthorize,
}

impl InstructionConfig for AuthorizeConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let authority = self
            .override_authority
            .unwrap_or(match self.stake_authorize {
                StakeAuthorize::Staker => &ctx.staker,
                StakeAuthorize::Withdrawer => &ctx.withdrawer,
            });
        ixn::authorize(
            self.stake.0,
            authority,
            self.new_authority,
            self.stake_authorize,
            None,
        )
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![(*self.stake.0, self.stake.1.clone())]
    }
}

pub struct AuthorizeCheckedConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub authority: &'a Pubkey,
    pub new_authority: &'a Pubkey,
    pub stake_authorize: StakeAuthorize,
    pub custodian: Option<&'a Pubkey>,
}

impl InstructionConfig for AuthorizeCheckedConfig<'_> {
    fn build_instruction(&self, _ctx: &StakeTestContext) -> Instruction {
        ixn::authorize_checked(
            self.stake.0,
            self.authority,
            self.new_authority,
            self.stake_authorize,
            self.custodian,
        )
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![(*self.stake.0, self.stake.1.clone())]
    }
}

pub struct AuthorizeCheckedWithSeedConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub authority_base: &'a Pubkey,
    pub authority_seed: String,
    pub authority_owner: &'a Pubkey,
    pub new_authority: &'a Pubkey,
    pub stake_authorize: StakeAuthorize,
    pub custodian: Option<&'a Pubkey>,
}

impl InstructionConfig for AuthorizeCheckedWithSeedConfig<'_> {
    fn build_instruction(&self, _ctx: &StakeTestContext) -> Instruction {
        ixn::authorize_checked_with_seed(
            self.stake.0,
            self.authority_base,
            self.authority_seed.clone(),
            self.authority_owner,
            self.new_authority,
            self.stake_authorize,
            self.custodian,
        )
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

pub struct MergeConfig<'a> {
    pub destination: (&'a Pubkey, &'a AccountSharedData),
    pub source: (&'a Pubkey, &'a AccountSharedData),
}

impl InstructionConfig for MergeConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let instructions = ixn::merge(self.destination.0, self.source.0, &ctx.staker);
        instructions[0].clone() // Merge returns a Vec, use first instruction
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![
            (*self.destination.0, self.destination.1.clone()),
            (*self.source.0, self.source.1.clone()),
        ]
    }
}

pub struct MoveLamportsConfig<'a> {
    pub source: (&'a Pubkey, &'a AccountSharedData),
    pub destination: (&'a Pubkey, &'a AccountSharedData),
    pub amount: u64,
    /// Override signer for testing wrong signer scenarios (defaults to ctx.staker)
    pub override_signer: Option<&'a Pubkey>,
}

impl<'a> MoveLamportsConfig<'a> {
    /// Helper to get the default source vote account from context
    pub fn with_default_vote(self, ctx: &'a StakeTestContext) -> MoveLamportsFullConfig<'a> {
        MoveLamportsFullConfig {
            source: self.source,
            destination: self.destination,
            override_signer: self.override_signer,
            amount: self.amount,
            source_vote: (
                ctx.vote_account.as_ref().expect("vote_account required"),
                ctx.vote_account_data
                    .as_ref()
                    .expect("vote_account_data required"),
            ),
            dest_vote: None,
        }
    }
}

impl InstructionConfig for MoveLamportsConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let signer = self.override_signer.unwrap_or(&ctx.staker);
        ixn::move_lamports(self.source.0, self.destination.0, signer, self.amount)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![
            (*self.source.0, self.source.1.clone()),
            (*self.destination.0, self.destination.1.clone()),
        ]
    }
}

pub struct MoveLamportsFullConfig<'a> {
    pub source: (&'a Pubkey, &'a AccountSharedData),
    pub destination: (&'a Pubkey, &'a AccountSharedData),
    pub amount: u64,
    /// Override signer for testing wrong signer scenarios (defaults to ctx.staker)
    pub override_signer: Option<&'a Pubkey>,
    pub source_vote: (&'a Pubkey, &'a AccountSharedData),
    pub dest_vote: Option<(&'a Pubkey, &'a AccountSharedData)>,
}

impl InstructionConfig for MoveLamportsFullConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let signer = self.override_signer.unwrap_or(&ctx.staker);
        ixn::move_lamports(self.source.0, self.destination.0, signer, self.amount)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        let mut accounts = vec![
            (*self.source.0, self.source.1.clone()),
            (*self.destination.0, self.destination.1.clone()),
            (*self.source_vote.0, self.source_vote.1.clone()),
        ];
        if let Some((vote_pk, vote_acc)) = self.dest_vote {
            accounts.push((*vote_pk, vote_acc.clone()));
        }
        accounts
    }
}

pub struct MoveStakeConfig<'a> {
    pub source: (&'a Pubkey, &'a AccountSharedData),
    pub destination: (&'a Pubkey, &'a AccountSharedData),
    pub amount: u64,
    /// Override signer for testing wrong signer scenarios (defaults to ctx.staker)
    pub override_signer: Option<&'a Pubkey>,
}

impl<'a> MoveStakeConfig<'a> {
    /// Helper to get the default source vote account from context
    pub fn with_default_vote(self, ctx: &'a StakeTestContext) -> MoveStakeWithVoteConfig<'a> {
        MoveStakeWithVoteConfig {
            source: self.source,
            destination: self.destination,
            override_signer: self.override_signer,
            amount: self.amount,
            source_vote: (
                ctx.vote_account.as_ref().expect("vote_account required"),
                ctx.vote_account_data
                    .as_ref()
                    .expect("vote_account_data required"),
            ),
            dest_vote: None,
        }
    }
}

impl InstructionConfig for MoveStakeConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let signer = self.override_signer.unwrap_or(&ctx.staker);
        ixn::move_stake(self.source.0, self.destination.0, signer, self.amount)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![
            (*self.source.0, self.source.1.clone()),
            (*self.destination.0, self.destination.1.clone()),
        ]
    }
}

pub struct MoveStakeWithVoteConfig<'a> {
    pub source: (&'a Pubkey, &'a AccountSharedData),
    pub destination: (&'a Pubkey, &'a AccountSharedData),
    pub amount: u64,
    /// Override signer for testing wrong signer scenarios (defaults to ctx.staker)
    pub override_signer: Option<&'a Pubkey>,
    pub source_vote: (&'a Pubkey, &'a AccountSharedData),
    pub dest_vote: Option<(&'a Pubkey, &'a AccountSharedData)>,
}

impl InstructionConfig for MoveStakeWithVoteConfig<'_> {
    fn build_instruction(&self, ctx: &StakeTestContext) -> Instruction {
        let signer = self.override_signer.unwrap_or(&ctx.staker);
        ixn::move_stake(self.source.0, self.destination.0, signer, self.amount)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        let mut accounts = vec![
            (*self.source.0, self.source.1.clone()),
            (*self.destination.0, self.destination.1.clone()),
            (*self.source_vote.0, self.source_vote.1.clone()),
        ];
        if let Some((vote_pk, vote_acc)) = self.dest_vote {
            accounts.push((*vote_pk, vote_acc.clone()));
        }
        accounts
    }
}

pub struct SetLockupCheckedConfig<'a> {
    pub stake: (&'a Pubkey, &'a AccountSharedData),
    pub lockup_args: &'a ixn::LockupArgs,
    pub custodian: &'a Pubkey,
}

impl InstructionConfig for SetLockupCheckedConfig<'_> {
    fn build_instruction(&self, _ctx: &StakeTestContext) -> Instruction {
        ixn::set_lockup_checked(self.stake.0, self.lockup_args, self.custodian)
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![(*self.stake.0, self.stake.1.clone())]
    }
}

pub struct SplitConfig<'a> {
    pub source: (&'a Pubkey, &'a AccountSharedData),
    pub destination: (&'a Pubkey, &'a AccountSharedData),
    pub amount: u64,
    pub signer: &'a Pubkey,
}

impl InstructionConfig for SplitConfig<'_> {
    fn build_instruction(&self, _ctx: &StakeTestContext) -> Instruction {
        let instructions = ixn::split(self.source.0, self.signer, self.amount, self.destination.0);
        instructions[2].clone() // The actual split instruction
    }

    fn build_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        vec![
            (*self.source.0, self.source.1.clone()),
            (*self.destination.0, self.destination.1.clone()),
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
