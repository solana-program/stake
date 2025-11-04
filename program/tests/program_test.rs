#![allow(clippy::arithmetic_side_effects)]

use {
    solana_account::Account as SolanaAccount,
    solana_clock::Clock,
    solana_instruction::Instruction,
    solana_keypair::Keypair,
    solana_program_entrypoint::ProgramResult,
    solana_program_error::ProgramError,
    solana_program_test::*,
    solana_pubkey::Pubkey,
    solana_sdk_ids::system_program,
    solana_signer::Signer,
    solana_stake_interface::{
        instruction::{self as ixn, LockupArgs},
        program::id,
        stake_history::StakeHistory,
        state::{Authorized, Lockup, Meta, Stake, StakeAuthorize, StakeStateV2},
    },
    solana_system_interface::instruction as system_instruction,
    solana_transaction::{Signers, Transaction, TransactionError},
    solana_vote_interface::{
        instruction as vote_instruction,
        state::{VoteInit, VoteStateV4},
    },
};

pub const USER_STARTING_LAMPORTS: u64 = 10_000_000_000_000; // 10k sol
pub const NO_SIGNERS: &[Keypair] = &[];

pub fn program_test() -> ProgramTest {
    program_test_without_features(&[])
}

pub fn program_test_without_features(feature_ids: &[Pubkey]) -> ProgramTest {
    let mut program_test = ProgramTest::default();
    program_test.prefer_bpf(true);

    for feature_id in feature_ids {
        program_test.deactivate_feature(*feature_id);
    }

    program_test.add_upgradeable_program_to_genesis("solana_stake_program", &id());

    program_test
}

#[derive(Debug, PartialEq)]
pub struct Accounts {
    pub validator: Keypair,
    pub voter: Keypair,
    pub withdrawer: Keypair,
    pub vote_account: Keypair,
}

impl Accounts {
    pub async fn initialize(&self, context: &mut ProgramTestContext) {
        let slot = context.genesis_config().epoch_schedule.first_normal_slot + 1;
        context.warp_to_slot(slot).unwrap();

        create_vote(
            context,
            &self.validator,
            &self.voter.pubkey(),
            &self.withdrawer.pubkey(),
            &self.vote_account,
        )
        .await;
    }
}

impl Default for Accounts {
    fn default() -> Self {
        let vote_account = Keypair::new();

        Self {
            validator: Keypair::new(),
            voter: Keypair::new(),
            withdrawer: Keypair::new(),
            vote_account,
        }
    }
}

pub async fn create_vote(
    context: &mut ProgramTestContext,
    validator: &Keypair,
    voter: &Pubkey,
    withdrawer: &Pubkey,
    vote_account: &Keypair,
) {
    let rent = context.banks_client.get_rent().await.unwrap();
    let rent_voter = rent.minimum_balance(VoteStateV4::size_of());

    let mut instructions = vec![system_instruction::create_account(
        &context.payer.pubkey(),
        &validator.pubkey(),
        rent.minimum_balance(0),
        0,
        &system_program::id(),
    )];
    instructions.append(&mut vote_instruction::create_account_with_config(
        &context.payer.pubkey(),
        &vote_account.pubkey(),
        &VoteInit {
            node_pubkey: validator.pubkey(),
            authorized_voter: *voter,
            authorized_withdrawer: *withdrawer,
            ..VoteInit::default()
        },
        rent_voter,
        vote_instruction::CreateVoteAccountConfig {
            space: VoteStateV4::size_of() as u64,
            ..Default::default()
        },
    ));

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[validator, vote_account, &context.payer],
        context.last_blockhash,
    );

    // ignore errors for idempotency
    let _ = context.banks_client.process_transaction(transaction).await;
}

pub async fn transfer(context: &mut ProgramTestContext, recipient: &Pubkey, amount: u64) {
    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &context.payer.pubkey(),
            recipient,
            amount,
        )],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}

pub async fn advance_epoch(context: &mut ProgramTestContext) {
    refresh_blockhash(context).await;

    let root_slot = context.banks_client.get_root_slot().await.unwrap();
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    context.warp_to_slot(root_slot + slots_per_epoch).unwrap();
}

pub async fn refresh_blockhash(context: &mut ProgramTestContext) {
    context.last_blockhash = context
        .banks_client
        .get_new_latest_blockhash(&context.last_blockhash)
        .await
        .unwrap();
}

pub async fn get_account(banks_client: &mut BanksClient, pubkey: &Pubkey) -> SolanaAccount {
    banks_client
        .get_account(*pubkey)
        .await
        .expect("client error")
        .expect("account not found")
}

pub async fn get_stake_account(
    banks_client: &mut BanksClient,
    pubkey: &Pubkey,
) -> (Meta, Option<Stake>, u64) {
    let stake_account = get_account(banks_client, pubkey).await;
    let lamports = stake_account.lamports;
    match bincode::deserialize::<StakeStateV2>(&stake_account.data).unwrap() {
        StakeStateV2::Initialized(meta) => (meta, None, lamports),
        StakeStateV2::Stake(meta, stake, _) => (meta, Some(stake), lamports),
        StakeStateV2::Uninitialized => panic!("panic: uninitialized"),
        _ => unimplemented!(),
    }
}

pub async fn get_stake_account_rent(banks_client: &mut BanksClient) -> u64 {
    let rent = banks_client.get_rent().await.unwrap();
    rent.minimum_balance(std::mem::size_of::<StakeStateV2>())
}

pub async fn get_effective_stake(banks_client: &mut BanksClient, pubkey: &Pubkey) -> u64 {
    let clock = banks_client.get_sysvar::<Clock>().await.unwrap();
    let stake_history = banks_client.get_sysvar::<StakeHistory>().await.unwrap();
    let stake_account = get_account(banks_client, pubkey).await;
    match bincode::deserialize::<StakeStateV2>(&stake_account.data).unwrap() {
        StakeStateV2::Stake(_, stake, _) => {
            stake
                .delegation
                .stake_activating_and_deactivating(clock.epoch, &stake_history, Some(0))
                .effective
        }
        _ => 0,
    }
}

pub async fn create_independent_stake_account(
    context: &mut ProgramTestContext,
    authorized: &Authorized,
    stake_amount: u64,
) -> Pubkey {
    create_independent_stake_account_with_lockup(
        context,
        authorized,
        &Lockup::default(),
        stake_amount,
    )
    .await
}

pub async fn create_independent_stake_account_with_lockup(
    context: &mut ProgramTestContext,
    authorized: &Authorized,
    lockup: &Lockup,
    stake_amount: u64,
) -> Pubkey {
    let stake = Keypair::new();
    let lamports = get_stake_account_rent(&mut context.banks_client).await + stake_amount;

    let instructions = vec![
        system_instruction::create_account(
            &context.payer.pubkey(),
            &stake.pubkey(),
            lamports,
            std::mem::size_of::<StakeStateV2>() as u64,
            &id(),
        ),
        ixn::initialize(&stake.pubkey(), authorized, lockup),
    ];

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer, &stake],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    stake.pubkey()
}

pub async fn create_blank_stake_account(context: &mut ProgramTestContext) -> Pubkey {
    let stake = Keypair::new();
    create_blank_stake_account_from_keypair(context, &stake, false).await
}

pub async fn create_closed_stake_account(context: &mut ProgramTestContext) -> Pubkey {
    let stake = Keypair::new();
    create_blank_stake_account_from_keypair(context, &stake, true).await
}

pub async fn create_blank_stake_account_from_keypair(
    context: &mut ProgramTestContext,
    stake: &Keypair,
    is_closed: bool,
) -> Pubkey {
    // lamports in a "closed" account is arbitrary, a real one via split/merge/withdraw would have 0
    let lamports = get_stake_account_rent(&mut context.banks_client).await;

    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::create_account(
            &context.payer.pubkey(),
            &stake.pubkey(),
            lamports,
            if is_closed {
                0
            } else {
                StakeStateV2::size_of() as u64
            },
            &id(),
        )],
        Some(&context.payer.pubkey()),
        &[&context.payer, stake],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    stake.pubkey()
}

pub async fn process_instruction<T: Signers + ?Sized>(
    context: &mut ProgramTestContext,
    instruction: &Instruction,
    additional_signers: &T,
) -> ProgramResult {
    let mut transaction =
        Transaction::new_with_payer(&[instruction.clone()], Some(&context.payer.pubkey()));

    transaction.partial_sign(&[&context.payer], context.last_blockhash);
    transaction.sign(additional_signers, context.last_blockhash);

    match context.banks_client.process_transaction(transaction).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // banks client error -> transaction error -> instruction error -> program error
            match e.unwrap() {
                TransactionError::InstructionError(_, e) => Err(e.try_into().unwrap()),
                TransactionError::InsufficientFundsForRent { .. } => {
                    Err(ProgramError::InsufficientFunds)
                }
                _ => panic!("couldnt convert {:?} to ProgramError", e),
            }
        }
    }
}

pub async fn process_instruction_test_missing_signers(
    context: &mut ProgramTestContext,
    instruction: &Instruction,
    additional_signers: &Vec<&Keypair>,
) {
    // remove every signer one by one and ensure we always fail
    for i in 0..instruction.accounts.len() {
        if instruction.accounts[i].is_signer {
            let mut instruction = instruction.clone();
            instruction.accounts[i].is_signer = false;
            let reduced_signers: Vec<_> = additional_signers
                .iter()
                .filter(|s| s.pubkey() != instruction.accounts[i].pubkey)
                .collect();

            let e = process_instruction(context, &instruction, &reduced_signers)
                .await
                .unwrap_err();
            assert_eq!(e, ProgramError::MissingRequiredSignature);
        }
    }

    // now make sure the instruction succeeds
    process_instruction(context, instruction, additional_signers)
        .await
        .unwrap();
}

#[tokio::test]
async fn program_test_stake_checked_instructions() {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();
    let authorized_keypair = Keypair::new();
    let seed_base_keypair = Keypair::new();
    let custodian_keypair = Keypair::new();

    let staker = staker_keypair.pubkey();
    let withdrawer = withdrawer_keypair.pubkey();
    let authorized = authorized_keypair.pubkey();
    let seed_base = seed_base_keypair.pubkey();
    let custodian = custodian_keypair.pubkey();

    let seed = "test seed";
    let seeded_address = Pubkey::create_with_seed(&seed_base, seed, &system_program::id()).unwrap();

    // Test AuthorizeChecked with non-signing staker
    let stake =
        create_independent_stake_account(&mut context, &Authorized { staker, withdrawer }, 0).await;
    let instruction =
        ixn::authorize_checked(&stake, &staker, &authorized, StakeAuthorize::Staker, None);

    process_instruction_test_missing_signers(
        &mut context,
        &instruction,
        &vec![&staker_keypair, &authorized_keypair],
    )
    .await;

    // Test AuthorizeChecked with non-signing withdrawer
    let stake =
        create_independent_stake_account(&mut context, &Authorized { staker, withdrawer }, 0).await;
    let instruction = ixn::authorize_checked(
        &stake,
        &withdrawer,
        &authorized,
        StakeAuthorize::Withdrawer,
        None,
    );

    process_instruction_test_missing_signers(
        &mut context,
        &instruction,
        &vec![&withdrawer_keypair, &authorized_keypair],
    )
    .await;

    // Test AuthorizeCheckedWithSeed with non-signing authority
    for authority_type in [StakeAuthorize::Staker, StakeAuthorize::Withdrawer] {
        let stake =
            create_independent_stake_account(&mut context, &Authorized::auto(&seeded_address), 0)
                .await;
        let instruction = ixn::authorize_checked_with_seed(
            &stake,
            &seed_base,
            seed.to_string(),
            &system_program::id(),
            &authorized,
            authority_type,
            None,
        );

        process_instruction_test_missing_signers(
            &mut context,
            &instruction,
            &vec![&seed_base_keypair, &authorized_keypair],
        )
        .await;
    }

    // Test SetLockupChecked with non-signing lockup custodian
    let stake =
        create_independent_stake_account(&mut context, &Authorized { staker, withdrawer }, 0).await;
    let instruction = ixn::set_lockup_checked(
        &stake,
        &LockupArgs {
            unix_timestamp: None,
            epoch: Some(1),
            custodian: Some(custodian),
        },
        &withdrawer,
    );

    process_instruction_test_missing_signers(
        &mut context,
        &instruction,
        &vec![&withdrawer_keypair, &custodian_keypair],
    )
    .await;
}

#[tokio::test]
async fn program_test_authorize() {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;

    let stakers: [_; 3] = std::array::from_fn(|_| Keypair::new());
    let withdrawers: [_; 3] = std::array::from_fn(|_| Keypair::new());

    let stake_keypair = Keypair::new();
    let stake = create_blank_stake_account_from_keypair(&mut context, &stake_keypair, false).await;

    // authorize uninitialized fails
    for (authority, authority_type) in [
        (&stakers[0], StakeAuthorize::Staker),
        (&withdrawers[0], StakeAuthorize::Withdrawer),
    ] {
        let instruction = ixn::authorize(&stake, &stake, &authority.pubkey(), authority_type, None);
        let e = process_instruction(&mut context, &instruction, &vec![&stake_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InvalidAccountData);
    }

    let authorized = Authorized {
        staker: stakers[0].pubkey(),
        withdrawer: withdrawers[0].pubkey(),
    };

    let instruction = ixn::initialize(&stake, &authorized, &Lockup::default());
    process_instruction(&mut context, &instruction, NO_SIGNERS)
        .await
        .unwrap();

    // changing authority works
    for (old_authority, new_authority, authority_type) in [
        (&stakers[0], &stakers[1], StakeAuthorize::Staker),
        (&withdrawers[0], &withdrawers[1], StakeAuthorize::Withdrawer),
    ] {
        let instruction = ixn::authorize(
            &stake,
            &old_authority.pubkey(),
            &new_authority.pubkey(),
            authority_type,
            None,
        );
        process_instruction_test_missing_signers(&mut context, &instruction, &vec![old_authority])
            .await;

        let (meta, _, _) = get_stake_account(&mut context.banks_client, &stake).await;
        let actual_authority = match authority_type {
            StakeAuthorize::Staker => meta.authorized.staker,
            StakeAuthorize::Withdrawer => meta.authorized.withdrawer,
        };
        assert_eq!(actual_authority, new_authority.pubkey());
    }

    // old authority no longer works
    for (old_authority, new_authority, authority_type) in [
        (&stakers[0], Pubkey::new_unique(), StakeAuthorize::Staker),
        (
            &withdrawers[0],
            Pubkey::new_unique(),
            StakeAuthorize::Withdrawer,
        ),
    ] {
        let instruction = ixn::authorize(
            &stake,
            &old_authority.pubkey(),
            &new_authority,
            authority_type,
            None,
        );
        let e = process_instruction(&mut context, &instruction, &vec![old_authority])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);
    }

    // changing authority again works
    for (old_authority, new_authority, authority_type) in [
        (&stakers[1], &stakers[2], StakeAuthorize::Staker),
        (&withdrawers[1], &withdrawers[2], StakeAuthorize::Withdrawer),
    ] {
        let instruction = ixn::authorize(
            &stake,
            &old_authority.pubkey(),
            &new_authority.pubkey(),
            authority_type,
            None,
        );
        process_instruction_test_missing_signers(&mut context, &instruction, &vec![old_authority])
            .await;

        let (meta, _, _) = get_stake_account(&mut context.banks_client, &stake).await;
        let actual_authority = match authority_type {
            StakeAuthorize::Staker => meta.authorized.staker,
            StakeAuthorize::Withdrawer => meta.authorized.withdrawer,
        };
        assert_eq!(actual_authority, new_authority.pubkey());
    }

    // changing withdrawer using staker fails
    let instruction = ixn::authorize(
        &stake,
        &stakers[2].pubkey(),
        &Pubkey::new_unique(),
        StakeAuthorize::Withdrawer,
        None,
    );
    let e = process_instruction(&mut context, &instruction, &vec![&stakers[2]])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::MissingRequiredSignature);

    // changing staker using withdrawer is fine
    let instruction = ixn::authorize(
        &stake,
        &withdrawers[2].pubkey(),
        &stakers[0].pubkey(),
        StakeAuthorize::Staker,
        None,
    );
    process_instruction_test_missing_signers(&mut context, &instruction, &vec![&withdrawers[2]])
        .await;

    let (meta, _, _) = get_stake_account(&mut context.banks_client, &stake).await;
    assert_eq!(meta.authorized.staker, stakers[0].pubkey());

    // withdraw using staker fails
    for staker in stakers {
        let recipient = Pubkey::new_unique();
        let instruction = ixn::withdraw(
            &stake,
            &staker.pubkey(),
            &recipient,
            rent_exempt_reserve,
            None,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StakeLifecycle {
    Uninitialized = 0,
    Initialized,
    Activating,
    Active,
    Deactivating,
    Deactive,
    Closed,
}
impl StakeLifecycle {
    // (stake, staker, withdrawer)
    pub async fn new_stake_account(
        self,
        context: &mut ProgramTestContext,
        vote_account: &Pubkey,
        staked_amount: u64,
    ) -> (Keypair, Keypair, Keypair) {
        let stake_keypair = Keypair::new();
        let staker_keypair = Keypair::new();
        let withdrawer_keypair = Keypair::new();

        self.new_stake_account_fully_specified(
            context,
            vote_account,
            staked_amount,
            &stake_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &Lockup::default(),
        )
        .await;

        (stake_keypair, staker_keypair, withdrawer_keypair)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn new_stake_account_fully_specified(
        self,
        context: &mut ProgramTestContext,
        vote_account: &Pubkey,
        staked_amount: u64,
        stake_keypair: &Keypair,
        staker_keypair: &Keypair,
        withdrawer_keypair: &Keypair,
        lockup: &Lockup,
    ) {
        let is_closed = self == StakeLifecycle::Closed;

        let stake =
            create_blank_stake_account_from_keypair(context, stake_keypair, is_closed).await;
        if staked_amount > 0 {
            transfer(context, &stake, staked_amount).await;
        }

        if is_closed {
            return;
        }

        let authorized = Authorized {
            staker: staker_keypair.pubkey(),
            withdrawer: withdrawer_keypair.pubkey(),
        };

        if self >= StakeLifecycle::Initialized {
            let instruction = ixn::initialize(&stake, &authorized, lockup);
            process_instruction(context, &instruction, NO_SIGNERS)
                .await
                .unwrap();
        }

        if self >= StakeLifecycle::Activating {
            let instruction = ixn::delegate_stake(&stake, &staker_keypair.pubkey(), vote_account);
            process_instruction(context, &instruction, &vec![staker_keypair])
                .await
                .unwrap();
        }

        if self >= StakeLifecycle::Active {
            advance_epoch(context).await;
            assert_eq!(
                get_effective_stake(&mut context.banks_client, &stake).await,
                staked_amount,
            );
        }

        if self >= StakeLifecycle::Deactivating {
            let instruction = ixn::deactivate_stake(&stake, &staker_keypair.pubkey());
            process_instruction(context, &instruction, &vec![staker_keypair])
                .await
                .unwrap();
        }

        if self == StakeLifecycle::Deactive {
            advance_epoch(context).await;
            assert_eq!(
                get_effective_stake(&mut context.banks_client, &stake).await,
                0,
            );
        }
    }

    // NOTE the program enforces that a deactive stake adheres to the split minimum,
    // albeit spuriously after solana-program/stake-program #1 is addressed,
    // Self::Deactive should move to false equivalently this could be combined
    // with withdraw_minimum_enforced into a function minimum_enforced
    pub fn split_minimum_enforced(&self) -> bool {
        match self {
            Self::Activating | Self::Active | Self::Deactivating | Self::Deactive => true,
            Self::Uninitialized | Self::Initialized | Self::Closed => false,
        }
    }

    pub fn withdraw_minimum_enforced(&self) -> bool {
        match self {
            Self::Activating | Self::Active | Self::Deactivating => true,
            Self::Uninitialized | Self::Initialized | Self::Deactive | Self::Closed => false,
        }
    }
}
