#![allow(clippy::arithmetic_side_effects)]

use {
    solana_program_test::*,
    solana_sdk::{
        account::Account as SolanaAccount,
        entrypoint::ProgramResult,
        instruction::Instruction,
        program_error::ProgramError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        signers::Signers,
        stake::{
            self,
            instruction::{self as ixn, LockupArgs, StakeError},
            state::{Authorized, Delegation, Lockup, Meta, Stake, StakeAuthorize, StakeStateV2},
        },
        system_instruction, system_program,
        sysvar::{clock::Clock, stake_history::StakeHistory},
        transaction::{Transaction, TransactionError},
        vote::{
            instruction as vote_instruction,
            state::{VoteInit, VoteState, VoteStateVersions},
        },
    },
    solana_stake_program::id,
    test_case::{test_case, test_matrix},
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
    let rent_voter = rent.minimum_balance(VoteState::size_of());

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
            space: VoteStateVersions::vote_state_size_of(true) as u64,
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
    rent.minimum_balance(std::mem::size_of::<stake::state::StakeStateV2>())
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

async fn get_minimum_delegation(context: &mut ProgramTestContext) -> u64 {
    let transaction = Transaction::new_signed_with_payer(
        &[stake::instruction::get_minimum_delegation()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    let mut data = context
        .banks_client
        .simulate_transaction(transaction)
        .await
        .unwrap()
        .simulation_details
        .unwrap()
        .return_data
        .unwrap()
        .data;
    data.resize(8, 0);

    data.try_into().map(u64::from_le_bytes).unwrap()
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
            std::mem::size_of::<stake::state::StakeStateV2>() as u64,
            &id(),
        ),
        stake::instruction::initialize(&stake.pubkey(), authorized, lockup),
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
    create_blank_stake_account_from_keypair(context, &stake).await
}

pub async fn create_blank_stake_account_from_keypair(
    context: &mut ProgramTestContext,
    stake: &Keypair,
) -> Pubkey {
    let lamports = get_stake_account_rent(&mut context.banks_client).await;

    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::create_account(
            &context.payer.pubkey(),
            &stake.pubkey(),
            lamports,
            StakeStateV2::size_of() as u64,
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

    // Test InitializeChecked with non-signing withdrawer
    let stake = create_blank_stake_account(&mut context).await;
    let instruction = ixn::initialize_checked(&stake, &Authorized { staker, withdrawer });

    process_instruction_test_missing_signers(
        &mut context,
        &instruction,
        &vec![&withdrawer_keypair],
    )
    .await;

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
async fn program_test_stake_initialize() {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;

    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();
    let custodian_keypair = Keypair::new();

    let staker = staker_keypair.pubkey();
    let withdrawer = withdrawer_keypair.pubkey();
    let custodian = custodian_keypair.pubkey();

    let authorized = Authorized { staker, withdrawer };

    let lockup = Lockup {
        epoch: 1,
        unix_timestamp: 0,
        custodian,
    };

    let stake = create_blank_stake_account(&mut context).await;
    let instruction = ixn::initialize(&stake, &authorized, &lockup);

    // should pass
    process_instruction(&mut context, &instruction, NO_SIGNERS)
        .await
        .unwrap();

    // check that we see what we expect
    let account = get_account(&mut context.banks_client, &stake).await;
    let stake_state: StakeStateV2 = bincode::deserialize(&account.data).unwrap();
    assert_eq!(
        stake_state,
        StakeStateV2::Initialized(Meta {
            authorized,
            rent_exempt_reserve,
            lockup,
        }),
    );

    // 2nd time fails, can't move it from anything other than uninit->init
    refresh_blockhash(&mut context).await;
    let e = process_instruction(&mut context, &instruction, NO_SIGNERS)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);

    // not enough balance for rent
    let stake = Pubkey::new_unique();
    let account = SolanaAccount {
        lamports: rent_exempt_reserve / 2,
        data: vec![0; StakeStateV2::size_of()],
        owner: id(),
        executable: false,
        rent_epoch: 1000,
    };
    context.set_account(&stake, &account.into());

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let e = process_instruction(&mut context, &instruction, NO_SIGNERS)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InsufficientFunds);

    // incorrect account sizes
    let stake_keypair = Keypair::new();
    let stake = stake_keypair.pubkey();

    let instruction = system_instruction::create_account(
        &context.payer.pubkey(),
        &stake,
        rent_exempt_reserve * 2,
        StakeStateV2::size_of() as u64 + 1,
        &id(),
    );
    process_instruction(&mut context, &instruction, &vec![&stake_keypair])
        .await
        .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let e = process_instruction(&mut context, &instruction, NO_SIGNERS)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);

    let stake_keypair = Keypair::new();
    let stake = stake_keypair.pubkey();

    let instruction = system_instruction::create_account(
        &context.payer.pubkey(),
        &stake,
        rent_exempt_reserve,
        StakeStateV2::size_of() as u64 - 1,
        &id(),
    );
    process_instruction(&mut context, &instruction, &vec![&stake_keypair])
        .await
        .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let e = process_instruction(&mut context, &instruction, NO_SIGNERS)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);
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
    let stake = create_blank_stake_account_from_keypair(&mut context, &stake_keypair).await;

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

#[tokio::test]
async fn program_test_stake_delegate() {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let vote_account2 = Keypair::new();
    create_vote(
        &mut context,
        &Keypair::new(),
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        &vote_account2,
    )
    .await;

    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();

    let staker = staker_keypair.pubkey();
    let withdrawer = withdrawer_keypair.pubkey();

    let authorized = Authorized { staker, withdrawer };

    let vote_state_credits = 100;
    context.increment_vote_account_credits(&accounts.vote_account.pubkey(), vote_state_credits);
    let minimum_delegation = get_minimum_delegation(&mut context).await;

    let stake =
        create_independent_stake_account(&mut context, &authorized, minimum_delegation).await;
    let instruction = ixn::delegate_stake(&stake, &staker, &accounts.vote_account.pubkey());

    process_instruction_test_missing_signers(&mut context, &instruction, &vec![&staker_keypair])
        .await;

    // verify that delegate() looks right
    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
    let (_, stake_data, _) = get_stake_account(&mut context.banks_client, &stake).await;
    assert_eq!(
        stake_data.unwrap(),
        Stake {
            delegation: Delegation {
                voter_pubkey: accounts.vote_account.pubkey(),
                stake: minimum_delegation,
                activation_epoch: clock.epoch,
                deactivation_epoch: u64::MAX,
                ..Delegation::default()
            },
            credits_observed: vote_state_credits,
        }
    );

    // verify that delegate fails as stake is active and not deactivating
    advance_epoch(&mut context).await;
    let instruction = ixn::delegate_stake(&stake, &staker, &accounts.vote_account.pubkey());
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, StakeError::TooSoonToRedelegate.into());

    // deactivate
    let instruction = ixn::deactivate_stake(&stake, &staker);
    process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap();

    // verify that delegate to a different vote account fails during deactivation
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2.pubkey());
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, StakeError::TooSoonToRedelegate.into());

    // verify that delegate succeeds to same vote account when stake is deactivating
    refresh_blockhash(&mut context).await;
    let instruction = ixn::delegate_stake(&stake, &staker, &accounts.vote_account.pubkey());
    process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap();

    // verify that deactivation has been cleared
    let (_, stake_data, _) = get_stake_account(&mut context.banks_client, &stake).await;
    assert_eq!(stake_data.unwrap().delegation.deactivation_epoch, u64::MAX);

    // verify that delegate to a different vote account fails if stake is still
    // active
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2.pubkey());
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, StakeError::TooSoonToRedelegate.into());

    // delegate still fails after stake is fully activated; redelegate is not
    // supported
    advance_epoch(&mut context).await;
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2.pubkey());
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, StakeError::TooSoonToRedelegate.into());

    // delegate to spoofed vote account fails (not owned by vote program)
    let mut fake_vote_account =
        get_account(&mut context.banks_client, &accounts.vote_account.pubkey()).await;
    fake_vote_account.owner = Pubkey::new_unique();
    let fake_vote_address = Pubkey::new_unique();
    context.set_account(&fake_vote_address, &fake_vote_account.into());

    let stake =
        create_independent_stake_account(&mut context, &authorized, minimum_delegation).await;
    let instruction = ixn::delegate_stake(&stake, &staker, &fake_vote_address);

    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::IncorrectProgramId);

    // delegate stake program-owned non-stake account fails
    let rewards_pool_address = Pubkey::new_unique();
    let rewards_pool = SolanaAccount {
        lamports: get_stake_account_rent(&mut context.banks_client).await,
        data: bincode::serialize(&StakeStateV2::RewardsPool)
            .unwrap()
            .to_vec(),
        owner: id(),
        executable: false,
        rent_epoch: u64::MAX,
    };
    context.set_account(&rewards_pool_address, &rewards_pool.into());

    let instruction = ixn::delegate_stake(
        &rewards_pool_address,
        &staker,
        &accounts.vote_account.pubkey(),
    );

    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StakeLifecycle {
    Uninitialized = 0,
    Initialized,
    Activating,
    Active,
    Deactivating,
    Deactive,
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
        let authorized = Authorized {
            staker: staker_keypair.pubkey(),
            withdrawer: withdrawer_keypair.pubkey(),
        };

        let stake = create_blank_stake_account_from_keypair(context, stake_keypair).await;
        if staked_amount > 0 {
            transfer(context, &stake, staked_amount).await;
        }

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
            Self::Uninitialized | Self::Initialized => false,
        }
    }

    pub fn withdraw_minimum_enforced(&self) -> bool {
        match self {
            Self::Activating | Self::Active | Self::Deactivating => true,
            Self::Uninitialized | Self::Initialized | Self::Deactive => false,
        }
    }
}

#[test_case(StakeLifecycle::Uninitialized; "uninitialized")]
#[test_case(StakeLifecycle::Initialized; "initialized")]
#[test_case(StakeLifecycle::Activating; "activating")]
#[test_case(StakeLifecycle::Active; "active")]
#[test_case(StakeLifecycle::Deactivating; "deactivating")]
#[test_case(StakeLifecycle::Deactive; "deactive")]
#[tokio::test]
async fn program_test_split(split_source_type: StakeLifecycle) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let staked_amount = minimum_delegation * 2;

    let (split_source_keypair, staker_keypair, _) = split_source_type
        .new_stake_account(&mut context, &accounts.vote_account.pubkey(), staked_amount)
        .await;

    let split_source = split_source_keypair.pubkey();
    let split_dest = create_blank_stake_account(&mut context).await;

    let signers = match split_source_type {
        StakeLifecycle::Uninitialized => vec![&split_source_keypair],
        _ => vec![&staker_keypair],
    };

    // fail, split more than available (even if not active, would kick source out of
    // rent exemption)
    let instruction = &ixn::split(
        &split_source,
        &signers[0].pubkey(),
        staked_amount + 1,
        &split_dest,
    )[2];

    let e = process_instruction(&mut context, instruction, &signers)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InsufficientFunds);

    // an active or transitioning stake account cannot have less than the minimum
    // delegation note this is NOT dependent on the minimum delegation feature.
    // there was ALWAYS a minimum. it was one lamport!
    if split_source_type.split_minimum_enforced() {
        // zero split fails
        let instruction = &ixn::split(&split_source, &signers[0].pubkey(), 0, &split_dest)[2];
        let e = process_instruction(&mut context, instruction, &signers)
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InsufficientFunds);

        // underfunded destination fails
        let instruction = &ixn::split(
            &split_source,
            &signers[0].pubkey(),
            minimum_delegation - 1,
            &split_dest,
        )[2];

        let e = process_instruction(&mut context, instruction, &signers)
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InsufficientFunds);

        // underfunded source fails
        let instruction = &ixn::split(
            &split_source,
            &signers[0].pubkey(),
            minimum_delegation + 1,
            &split_dest,
        )[2];

        let e = process_instruction(&mut context, instruction, &signers)
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InsufficientFunds);
    }

    // split to non-owned account fails
    let mut fake_split_dest_account = get_account(&mut context.banks_client, &split_dest).await;
    fake_split_dest_account.owner = Pubkey::new_unique();
    let fake_split_dest = Pubkey::new_unique();
    context.set_account(&fake_split_dest, &fake_split_dest_account.into());

    let instruction = &ixn::split(
        &split_source,
        &signers[0].pubkey(),
        staked_amount / 2,
        &fake_split_dest,
    )[2];

    let e = process_instruction(&mut context, instruction, &signers)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountOwner);

    // success
    let instruction = &ixn::split(
        &split_source,
        &signers[0].pubkey(),
        staked_amount / 2,
        &split_dest,
    )[2];
    process_instruction_test_missing_signers(&mut context, instruction, &signers).await;

    // source lost split amount
    let source_lamports = get_account(&mut context.banks_client, &split_source)
        .await
        .lamports;
    assert_eq!(source_lamports, staked_amount / 2 + rent_exempt_reserve);

    // destination gained split amount
    let dest_lamports = get_account(&mut context.banks_client, &split_dest)
        .await
        .lamports;
    assert_eq!(dest_lamports, staked_amount / 2 + rent_exempt_reserve);

    // destination meta has been set properly if ever delegated
    if split_source_type >= StakeLifecycle::Initialized {
        let (source_meta, source_stake, _) =
            get_stake_account(&mut context.banks_client, &split_source).await;
        let (dest_meta, dest_stake, _) =
            get_stake_account(&mut context.banks_client, &split_dest).await;
        assert_eq!(dest_meta, source_meta);

        // delegations are set properly if activating or active
        if split_source_type >= StakeLifecycle::Activating
            && split_source_type < StakeLifecycle::Deactive
        {
            assert_eq!(source_stake.unwrap().delegation.stake, staked_amount / 2);
            assert_eq!(dest_stake.unwrap().delegation.stake, staked_amount / 2);
        }
    }

    // nothing has been deactivated if active
    if split_source_type >= StakeLifecycle::Active && split_source_type < StakeLifecycle::Deactive {
        assert_eq!(
            get_effective_stake(&mut context.banks_client, &split_source).await,
            staked_amount / 2,
        );

        assert_eq!(
            get_effective_stake(&mut context.banks_client, &split_dest).await,
            staked_amount / 2,
        );
    }
}

#[test_case(StakeLifecycle::Uninitialized; "uninitialized")]
#[test_case(StakeLifecycle::Initialized; "initialized")]
#[test_case(StakeLifecycle::Activating; "activating")]
#[test_case(StakeLifecycle::Active; "active")]
#[test_case(StakeLifecycle::Deactivating; "deactivating")]
#[test_case(StakeLifecycle::Deactive; "deactive")]
#[tokio::test]
async fn program_test_withdraw_stake(withdraw_source_type: StakeLifecycle) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let stake_rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let staked_amount = minimum_delegation;

    let wallet_rent_exempt_reserve = context
        .banks_client
        .get_rent()
        .await
        .unwrap()
        .minimum_balance(0);

    let (withdraw_source_keypair, _, withdrawer_keypair) = withdraw_source_type
        .new_stake_account(&mut context, &accounts.vote_account.pubkey(), staked_amount)
        .await;
    let withdraw_source = withdraw_source_keypair.pubkey();

    let recipient = Pubkey::new_unique();
    transfer(&mut context, &recipient, wallet_rent_exempt_reserve).await;

    let signers = match withdraw_source_type {
        StakeLifecycle::Uninitialized => vec![&withdraw_source_keypair],
        _ => vec![&withdrawer_keypair],
    };

    // withdraw that would end rent-exemption always fails
    let instruction = ixn::withdraw(
        &withdraw_source,
        &signers[0].pubkey(),
        &recipient,
        staked_amount + 1,
        None,
    );
    let e = process_instruction(&mut context, &instruction, &signers)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InsufficientFunds);

    if withdraw_source_type.withdraw_minimum_enforced() {
        // withdraw active or activating stake fails
        let instruction = ixn::withdraw(
            &withdraw_source,
            &signers[0].pubkey(),
            &recipient,
            staked_amount,
            None,
        );
        let e = process_instruction(&mut context, &instruction, &signers)
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InsufficientFunds);

        // grant rewards
        let reward_amount = 10;
        transfer(&mut context, &withdraw_source, reward_amount).await;

        // withdraw in excess of rewards is not allowed
        let instruction = ixn::withdraw(
            &withdraw_source,
            &signers[0].pubkey(),
            &recipient,
            reward_amount + 1,
            None,
        );
        let e = process_instruction(&mut context, &instruction, &signers)
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InsufficientFunds);

        // withdraw rewards is allowed
        let instruction = ixn::withdraw(
            &withdraw_source,
            &signers[0].pubkey(),
            &recipient,
            reward_amount,
            None,
        );
        process_instruction_test_missing_signers(&mut context, &instruction, &signers).await;

        let recipient_lamports = get_account(&mut context.banks_client, &recipient)
            .await
            .lamports;
        assert_eq!(
            recipient_lamports,
            reward_amount + wallet_rent_exempt_reserve,
        );
    } else {
        // withdraw that leaves rent behind is allowed
        let instruction = ixn::withdraw(
            &withdraw_source,
            &signers[0].pubkey(),
            &recipient,
            staked_amount,
            None,
        );
        process_instruction_test_missing_signers(&mut context, &instruction, &signers).await;

        let recipient_lamports = get_account(&mut context.banks_client, &recipient)
            .await
            .lamports;
        assert_eq!(
            recipient_lamports,
            staked_amount + wallet_rent_exempt_reserve,
        );

        // full withdraw is allowed
        refresh_blockhash(&mut context).await;
        transfer(&mut context, &withdraw_source, staked_amount).await;

        let recipient = Pubkey::new_unique();
        transfer(&mut context, &recipient, wallet_rent_exempt_reserve).await;

        let instruction = ixn::withdraw(
            &withdraw_source,
            &signers[0].pubkey(),
            &recipient,
            staked_amount + stake_rent_exempt_reserve,
            None,
        );
        process_instruction_test_missing_signers(&mut context, &instruction, &signers).await;

        let recipient_lamports = get_account(&mut context.banks_client, &recipient)
            .await
            .lamports;
        assert_eq!(
            recipient_lamports,
            staked_amount + stake_rent_exempt_reserve + wallet_rent_exempt_reserve,
        );
    }

    // withdraw from program-owned non-stake not allowed
    let rewards_pool_address = Pubkey::new_unique();
    let rewards_pool = SolanaAccount {
        lamports: get_stake_account_rent(&mut context.banks_client).await + staked_amount,
        data: bincode::serialize(&StakeStateV2::RewardsPool)
            .unwrap()
            .to_vec(),
        owner: id(),
        executable: false,
        rent_epoch: u64::MAX,
    };
    context.set_account(&rewards_pool_address, &rewards_pool.into());

    let instruction = ixn::withdraw(
        &rewards_pool_address,
        &signers[0].pubkey(),
        &recipient,
        staked_amount,
        None,
    );
    let e = process_instruction(&mut context, &instruction, &signers)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);
}

#[test_case(false; "activating")]
#[test_case(true; "active")]
#[tokio::test]
async fn program_test_deactivate(activate: bool) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let minimum_delegation = get_minimum_delegation(&mut context).await;

    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();

    let staker = staker_keypair.pubkey();
    let withdrawer = withdrawer_keypair.pubkey();

    let authorized = Authorized { staker, withdrawer };

    let stake =
        create_independent_stake_account(&mut context, &authorized, minimum_delegation).await;

    // deactivating an undelegated account fails
    let instruction = ixn::deactivate_stake(&stake, &staker);
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);

    // delegate
    let instruction = ixn::delegate_stake(&stake, &staker, &accounts.vote_account.pubkey());
    process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap();

    if activate {
        advance_epoch(&mut context).await;
    } else {
        refresh_blockhash(&mut context).await;
    }

    // deactivate with withdrawer fails
    let instruction = ixn::deactivate_stake(&stake, &withdrawer);
    let e = process_instruction(&mut context, &instruction, &vec![&withdrawer_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::MissingRequiredSignature);

    // deactivate succeeds
    let instruction = ixn::deactivate_stake(&stake, &staker);
    process_instruction_test_missing_signers(&mut context, &instruction, &vec![&staker_keypair])
        .await;

    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
    let (_, stake_data, _) = get_stake_account(&mut context.banks_client, &stake).await;
    assert_eq!(
        stake_data.unwrap().delegation.deactivation_epoch,
        clock.epoch
    );

    // deactivate again fails
    refresh_blockhash(&mut context).await;

    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, StakeError::AlreadyDeactivated.into());

    advance_epoch(&mut context).await;

    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, StakeError::AlreadyDeactivated.into());
}

// XXX the original test_merge is a stupid test
// the real thing is test_merge_active_stake which actively controls clock and
// stake_history but im just trying to smoke test rn so lets do something
// simpler
#[test_matrix(
    [StakeLifecycle::Uninitialized, StakeLifecycle::Initialized, StakeLifecycle::Activating,
     StakeLifecycle::Active, StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Uninitialized, StakeLifecycle::Initialized, StakeLifecycle::Activating,
     StakeLifecycle::Active, StakeLifecycle::Deactivating, StakeLifecycle::Deactive]
)]
#[tokio::test]
async fn program_test_merge(merge_source_type: StakeLifecycle, merge_dest_type: StakeLifecycle) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let staked_amount = minimum_delegation;

    // stake accounts can be merged unconditionally:
    // * inactive and inactive
    // * inactive into activating
    // can be merged IF vote pubkey and credits match:
    // * active and active
    // * activating and activating, IF activating in the same epoch
    // in all cases, authorized and lockup also must match
    // uninitialized stakes cannot be merged at all
    let is_merge_allowed_by_type = match (merge_source_type, merge_dest_type) {
        // inactive and inactive
        (StakeLifecycle::Initialized, StakeLifecycle::Initialized)
        | (StakeLifecycle::Initialized, StakeLifecycle::Deactive)
        | (StakeLifecycle::Deactive, StakeLifecycle::Initialized)
        | (StakeLifecycle::Deactive, StakeLifecycle::Deactive) => true,

        // activating into inactive is also allowed although this isnt clear from docs
        (StakeLifecycle::Activating, StakeLifecycle::Initialized)
        | (StakeLifecycle::Activating, StakeLifecycle::Deactive) => true,

        // inactive into activating
        (StakeLifecycle::Initialized, StakeLifecycle::Activating)
        | (StakeLifecycle::Deactive, StakeLifecycle::Activating) => true,

        // active and active
        (StakeLifecycle::Active, StakeLifecycle::Active) => true,

        // activating and activating
        (StakeLifecycle::Activating, StakeLifecycle::Activating) => true,

        // better luck next time
        _ => false,
    };

    // create source first
    let (merge_source_keypair, _, _) = merge_source_type
        .new_stake_account(&mut context, &accounts.vote_account.pubkey(), staked_amount)
        .await;
    let merge_source = merge_source_keypair.pubkey();

    // retrieve its data
    let mut source_account = get_account(&mut context.banks_client, &merge_source).await;
    let mut source_stake_state: StakeStateV2 = bincode::deserialize(&source_account.data).unwrap();

    // create dest. this may mess source up if its in a transient state, but its
    // fine
    let (merge_dest_keypair, staker_keypair, withdrawer_keypair) = merge_dest_type
        .new_stake_account(&mut context, &accounts.vote_account.pubkey(), staked_amount)
        .await;
    let merge_dest = merge_dest_keypair.pubkey();

    // now we change source authorized to match dest
    // we can also true up the epoch if source should have been transient
    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
    match &mut source_stake_state {
        StakeStateV2::Initialized(ref mut meta) => {
            meta.authorized.staker = staker_keypair.pubkey();
            meta.authorized.withdrawer = withdrawer_keypair.pubkey();
        }
        StakeStateV2::Stake(ref mut meta, ref mut stake, _) => {
            meta.authorized.staker = staker_keypair.pubkey();
            meta.authorized.withdrawer = withdrawer_keypair.pubkey();

            match merge_source_type {
                StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
                StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
                _ => (),
            }
        }
        _ => (),
    }

    // and store
    source_account.data = bincode::serialize(&source_stake_state).unwrap();
    context.set_account(&merge_source, &source_account.into());

    // attempt to merge
    let instruction = ixn::merge(&merge_dest, &merge_source, &staker_keypair.pubkey())
        .into_iter()
        .next()
        .unwrap();

    // failure can result in various different errors... dont worry about it for now
    if is_merge_allowed_by_type {
        process_instruction_test_missing_signers(
            &mut context,
            &instruction,
            &vec![&staker_keypair],
        )
        .await;

        let dest_lamports = get_account(&mut context.banks_client, &merge_dest)
            .await
            .lamports;
        assert_eq!(dest_lamports, staked_amount * 2 + rent_exempt_reserve * 2);
    } else {
        process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
    }
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [false, true],
    [false, true]
)]
#[tokio::test]
async fn program_test_move_stake(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    full_move: bool,
    has_lockup: bool,
) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;

    // source has 2x minimum so we can easily test an unfunded destination
    let source_staked_amount = minimum_delegation * 2;

    // this is the amount of *staked* lamports for test checks
    // destinations may have excess lamports but these are *never* activated by move
    let dest_staked_amount = if move_dest_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    // test with and without lockup. both of these cases pass, we test failures
    // elsewhere
    let lockup = if has_lockup {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        let lockup = Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 100,
            custodian: Pubkey::new_unique(),
        };

        assert!(lockup.is_in_force(&clock, None));
        lockup
    } else {
        Lockup::default()
    };

    // we put an extra minimum in every account, unstaked, to test that no new
    // lamports activate name them here so our asserts are readable
    let source_excess = minimum_delegation;
    let dest_excess = minimum_delegation;

    let move_source_keypair = Keypair::new();
    let move_dest_keypair = Keypair::new();
    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();

    // create source stake
    move_source_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            source_staked_amount,
            &move_source_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_source = move_source_keypair.pubkey();
    let mut source_account = get_account(&mut context.banks_client, &move_source).await;
    let mut source_stake_state: StakeStateV2 = bincode::deserialize(&source_account.data).unwrap();

    // create dest stake with same authorities
    move_dest_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            minimum_delegation,
            &move_dest_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_dest = move_dest_keypair.pubkey();

    // true up source epoch if transient
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
    {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        if let StakeStateV2::Stake(_, ref mut stake, _) = &mut source_stake_state {
            match move_source_type {
                StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
                StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
                _ => (),
            }
        }

        source_account.data = bincode::serialize(&source_stake_state).unwrap();
        context.set_account(&move_source, &source_account.into());
    }

    // our inactive accounts have extra lamports, lets not let active feel left out
    if move_dest_type == StakeLifecycle::Active {
        transfer(&mut context, &move_dest, dest_excess).await;
    }

    // hey why not spread the love around to everyone
    transfer(&mut context, &move_source, source_excess).await;

    // alright first things first, clear out all the state failures
    match (move_source_type, move_dest_type) {
        // valid
        (StakeLifecycle::Active, StakeLifecycle::Initialized)
        | (StakeLifecycle::Active, StakeLifecycle::Active)
        | (StakeLifecycle::Active, StakeLifecycle::Deactive) => (),
        // invalid! get outta my test
        _ => {
            let instruction = ixn::move_stake(
                &move_source,
                &move_dest,
                &staker_keypair.pubkey(),
                if full_move {
                    source_staked_amount
                } else {
                    minimum_delegation
                },
            );

            // this is InvalidAccountData sometimes and Custom(5) sometimes but i dont care
            process_instruction(&mut context, &instruction, &vec![&staker_keypair])
                .await
                .unwrap_err();
            return;
        }
    }

    // the below checks are conceptually incoherent with a 1 lamport minimum
    // the undershoot fails successfully (but because its a zero move, not because
    // the destination ends underfunded) then the second one succeeds failedly
    // (because its a full move, so the "underfunded" source is actually closed)
    if minimum_delegation > 1 {
        // first for inactive accounts lets undershoot and fail for underfunded dest
        if move_dest_type != StakeLifecycle::Active {
            let instruction = ixn::move_stake(
                &move_source,
                &move_dest,
                &staker_keypair.pubkey(),
                minimum_delegation - 1,
            );

            let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
                .await
                .unwrap_err();
            assert_eq!(e, ProgramError::InvalidArgument);
        }

        // now lets overshoot and fail for underfunded source
        let instruction = ixn::move_stake(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation + 1,
        );

        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InvalidArgument);
    }

    // now we do it juuust right
    let instruction = ixn::move_stake(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        if full_move {
            source_staked_amount
        } else {
            minimum_delegation
        },
    );

    process_instruction_test_missing_signers(&mut context, &instruction, &vec![&staker_keypair])
        .await;

    if full_move {
        let (_, option_source_stake, source_lamports) =
            get_stake_account(&mut context.banks_client, &move_source).await;

        // source is deactivated and rent/excess stay behind
        assert!(option_source_stake.is_none());
        assert_eq!(source_lamports, source_excess + rent_exempt_reserve);

        let (_, Some(dest_stake), dest_lamports) =
            get_stake_account(&mut context.banks_client, &move_dest).await
        else {
            panic!("dest should be active")
        };
        let dest_effective_stake = get_effective_stake(&mut context.banks_client, &move_dest).await;

        // dest captured the entire source delegation, kept its rent/excess, didnt
        // activate its excess
        assert_eq!(
            dest_stake.delegation.stake,
            source_staked_amount + dest_staked_amount
        );
        assert_eq!(dest_effective_stake, dest_stake.delegation.stake);
        assert_eq!(
            dest_lamports,
            dest_effective_stake + dest_excess + rent_exempt_reserve
        );
    } else {
        let (_, Some(source_stake), source_lamports) =
            get_stake_account(&mut context.banks_client, &move_source).await
        else {
            panic!("source should be active")
        };
        let source_effective_stake =
            get_effective_stake(&mut context.banks_client, &move_source).await;

        // half of source delegation moved over, excess stayed behind
        assert_eq!(source_stake.delegation.stake, source_staked_amount / 2);
        assert_eq!(source_effective_stake, source_stake.delegation.stake);
        assert_eq!(
            source_lamports,
            source_effective_stake + source_excess + rent_exempt_reserve
        );

        let (_, Some(dest_stake), dest_lamports) =
            get_stake_account(&mut context.banks_client, &move_dest).await
        else {
            panic!("dest should be active")
        };
        let dest_effective_stake = get_effective_stake(&mut context.banks_client, &move_dest).await;

        // dest mirrors our observations
        assert_eq!(
            dest_stake.delegation.stake,
            source_staked_amount / 2 + dest_staked_amount
        );
        assert_eq!(dest_effective_stake, dest_stake.delegation.stake);
        assert_eq!(
            dest_lamports,
            dest_effective_stake + dest_excess + rent_exempt_reserve
        );
    }
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [false, true],
    [false, true]
)]
#[tokio::test]
async fn program_test_move_lamports(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    different_votes: bool,
    has_lockup: bool,
) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;

    // put minimum in both accounts if theyre active
    let source_staked_amount = if move_source_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    let dest_staked_amount = if move_dest_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    // test with and without lockup. both of these cases pass, we test failures
    // elsewhere
    let lockup = if has_lockup {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        let lockup = Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 100,
            custodian: Pubkey::new_unique(),
        };

        assert!(lockup.is_in_force(&clock, None));
        lockup
    } else {
        Lockup::default()
    };

    // we put an extra minimum in every account, unstaked, to test moving them
    let source_excess = minimum_delegation;
    let dest_excess = minimum_delegation;

    let move_source_keypair = Keypair::new();
    let move_dest_keypair = Keypair::new();
    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();

    // make a separate vote account if needed
    let dest_vote_account = if different_votes {
        let vote_account = Keypair::new();
        create_vote(
            &mut context,
            &Keypair::new(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &vote_account,
        )
        .await;

        vote_account.pubkey()
    } else {
        accounts.vote_account.pubkey()
    };

    // create source stake
    move_source_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            minimum_delegation,
            &move_source_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_source = move_source_keypair.pubkey();
    let mut source_account = get_account(&mut context.banks_client, &move_source).await;
    let mut source_stake_state: StakeStateV2 = bincode::deserialize(&source_account.data).unwrap();

    // create dest stake with same authorities
    move_dest_type
        .new_stake_account_fully_specified(
            &mut context,
            &dest_vote_account,
            minimum_delegation,
            &move_dest_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_dest = move_dest_keypair.pubkey();

    // true up source epoch if transient
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
    {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        if let StakeStateV2::Stake(_, ref mut stake, _) = &mut source_stake_state {
            match move_source_type {
                StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
                StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
                _ => (),
            }
        }

        source_account.data = bincode::serialize(&source_stake_state).unwrap();
        context.set_account(&move_source, &source_account.into());
    }

    // if we activated the initial amount we need to top up with the test lamports
    if move_source_type == StakeLifecycle::Active {
        transfer(&mut context, &move_source, source_excess).await;
    }
    if move_dest_type == StakeLifecycle::Active {
        transfer(&mut context, &move_dest, dest_excess).await;
    }

    // clear out state failures
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
        || move_dest_type == StakeLifecycle::Deactivating
    {
        let instruction = ixn::move_lamports(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            source_excess,
        );

        process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        return;
    }

    // overshoot and fail for underfunded source
    let instruction = ixn::move_lamports(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        source_excess + 1,
    );

    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidArgument);

    let (_, _, before_source_lamports) =
        get_stake_account(&mut context.banks_client, &move_source).await;
    let (_, _, before_dest_lamports) =
        get_stake_account(&mut context.banks_client, &move_dest).await;

    // now properly move the full excess
    let instruction = ixn::move_lamports(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        source_excess,
    );

    process_instruction_test_missing_signers(&mut context, &instruction, &vec![&staker_keypair])
        .await;

    let (_, _, after_source_lamports) =
        get_stake_account(&mut context.banks_client, &move_source).await;
    let source_effective_stake = get_effective_stake(&mut context.banks_client, &move_source).await;

    // source activation didnt change
    assert_eq!(source_effective_stake, source_staked_amount);

    // source lamports are right
    assert_eq!(
        after_source_lamports,
        before_source_lamports - minimum_delegation
    );
    assert_eq!(
        after_source_lamports,
        source_effective_stake + rent_exempt_reserve
    );

    let (_, _, after_dest_lamports) =
        get_stake_account(&mut context.banks_client, &move_dest).await;
    let dest_effective_stake = get_effective_stake(&mut context.banks_client, &move_dest).await;

    // dest activation didnt change
    assert_eq!(dest_effective_stake, dest_staked_amount);

    // dest lamports are right
    assert_eq!(
        after_dest_lamports,
        before_dest_lamports + minimum_delegation
    );
    assert_eq!(
        after_dest_lamports,
        dest_effective_stake + rent_exempt_reserve + source_excess + dest_excess
    );
}

#[test_matrix(
    [(StakeLifecycle::Active, StakeLifecycle::Uninitialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Initialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Uninitialized)],
    [false, true]
)]
#[tokio::test]
async fn program_test_move_uninitialized_fail(
    move_types: (StakeLifecycle, StakeLifecycle),
    move_lamports: bool,
) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let source_staked_amount = minimum_delegation * 2;

    let (move_source_type, move_dest_type) = move_types;

    let (move_source_keypair, staker_keypair, withdrawer_keypair) = move_source_type
        .new_stake_account(
            &mut context,
            &accounts.vote_account.pubkey(),
            source_staked_amount,
        )
        .await;
    let move_source = move_source_keypair.pubkey();

    let move_dest_keypair = Keypair::new();
    move_dest_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            0,
            &move_dest_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &Lockup::default(),
        )
        .await;
    let move_dest = move_dest_keypair.pubkey();

    let source_signer = if move_source_type == StakeLifecycle::Uninitialized {
        &move_source_keypair
    } else {
        &staker_keypair
    };

    let instruction = if move_lamports {
        ixn::move_lamports(
            &move_source,
            &move_dest,
            &source_signer.pubkey(),
            minimum_delegation,
        )
    } else {
        ixn::move_stake(
            &move_source,
            &move_dest,
            &source_signer.pubkey(),
            minimum_delegation,
        )
    };

    let e = process_instruction(&mut context, &instruction, &vec![source_signer])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [false, true]
)]
#[tokio::test]
async fn program_test_move_general_fail(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    move_lamports: bool,
) {
    // the test_matrix includes all valid source/dest combinations for MoveLamports
    // we dont test invalid combinations because they would fail regardless of the
    // fail cases we test here valid source/dest for MoveStake are a strict
    // subset of MoveLamports source must be active, and dest must be active or
    // inactive. so we skip the additional invalid MoveStake cases
    if !move_lamports
        && (move_source_type != StakeLifecycle::Active
            || move_dest_type == StakeLifecycle::Activating)
    {
        return;
    }

    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let source_staked_amount = minimum_delegation * 2;

    let in_force_lockup = {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 1_000_000,
            custodian: Pubkey::new_unique(),
        }
    };

    let mk_ixn = if move_lamports {
        ixn::move_lamports
    } else {
        ixn::move_stake
    };

    // we can reuse source but will need a lot of dest
    let (move_source_keypair, staker_keypair, withdrawer_keypair) = move_source_type
        .new_stake_account(
            &mut context,
            &accounts.vote_account.pubkey(),
            source_staked_amount,
        )
        .await;
    let move_source = move_source_keypair.pubkey();
    transfer(&mut context, &move_source, minimum_delegation).await;

    // self-move fails
    let instruction = mk_ixn(
        &move_source,
        &move_source,
        &staker_keypair.pubkey(),
        minimum_delegation,
    );
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidInstructionData);

    // first we make a "normal" move dest
    {
        let move_dest_keypair = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        // zero move fails
        let instruction = mk_ixn(&move_source, &move_dest, &staker_keypair.pubkey(), 0);
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InvalidArgument);

        // sign with withdrawer fails
        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &withdrawer_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&withdrawer_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);

        // good place to test source lockup
        let move_locked_source_keypair = Keypair::new();
        move_source_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                source_staked_amount,
                &move_locked_source_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &in_force_lockup,
            )
            .await;
        let move_locked_source = move_locked_source_keypair.pubkey();
        transfer(&mut context, &move_locked_source, minimum_delegation).await;

        let instruction = mk_ixn(
            &move_locked_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());
    }

    // staker mismatch
    {
        let move_dest_keypair = Keypair::new();
        let throwaway = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &throwaway,
                &withdrawer_keypair,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &throwaway.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&throwaway])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);
    }

    // withdrawer mismatch
    {
        let move_dest_keypair = Keypair::new();
        let throwaway = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &throwaway,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &throwaway.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&throwaway])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);
    }

    // dest lockup
    {
        let move_dest_keypair = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &in_force_lockup,
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());
    }

    // lastly we test different vote accounts for move_stake
    if !move_lamports && move_dest_type == StakeLifecycle::Active {
        let dest_vote_account_keypair = Keypair::new();
        create_vote(
            &mut context,
            &Keypair::new(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &dest_vote_account_keypair,
        )
        .await;

        let move_dest_keypair = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &dest_vote_account_keypair.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::VoteAddressMismatch.into());
    }
}
