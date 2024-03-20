#![allow(dead_code)]
#![allow(unused_imports)]

use {
    solana_program_test::*,
    solana_sdk::{
        account::Account as SolanaAccount,
        entrypoint::ProgramResult,
        feature_set::stake_raise_minimum_delegation_to_1_sol,
        hash::Hash,
        instruction::Instruction,
        native_token::LAMPORTS_PER_SOL,
        program_error::ProgramError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        signers::Signers,
        stake::{
            self,
            instruction::{
                self as ixn, LockupArgs, LockupCheckedArgs, StakeError, StakeInstruction,
            },
            state::{
                Authorized, Delegation, Lockup, Meta, Stake, StakeActivationStatus, StakeAuthorize,
                StakeStateV2,
            },
        },
        system_instruction, system_program,
        sysvar::{clock::Clock, rent::Rent, stake_history::StakeHistory},
        transaction::{Transaction, TransactionError},
    },
    solana_vote_program::{
        self, vote_instruction,
        vote_state::{self, VoteInit, VoteState, VoteStateVersions},
    },
    stake_program::processor::Processor,
    test_case::test_case,
};

pub const USER_STARTING_LAMPORTS: u64 = 10_000_000_000_000; // 10k sol

pub fn program_test(enable_minimum_delegation: bool) -> ProgramTest {
    let mut program_test = ProgramTest::default();
    // XXX do i not need this? program_test.prefer_bpf(false);

    program_test.add_program(
        "stake_program",
        stake_program::id(),
        processor!(Processor::process),
    );

    if !enable_minimum_delegation {
        program_test.deactivate_feature(stake_raise_minimum_delegation_to_1_sol::id());
    }

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
            &mut context.banks_client,
            &context.payer,
            &context.last_blockhash,
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
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    validator: &Keypair,
    voter: &Pubkey,
    withdrawer: &Pubkey,
    vote_account: &Keypair,
) {
    let rent = banks_client.get_rent().await.unwrap();
    let rent_voter = rent.minimum_balance(VoteState::size_of());

    let mut instructions = vec![system_instruction::create_account(
        &payer.pubkey(),
        &validator.pubkey(),
        rent.minimum_balance(0),
        0,
        &system_program::id(),
    )];
    instructions.append(&mut vote_instruction::create_account_with_config(
        &payer.pubkey(),
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
        Some(&payer.pubkey()),
        &[validator, vote_account, payer],
        *recent_blockhash,
    );

    // ignore errors for idempotency
    let _ = banks_client.process_transaction(transaction).await;
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
            &stake_program::id(),
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
            &stake_program::id(),
        )],
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

pub async fn test_instruction_with_missing_signers(
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

#[test_case(true; "all_enabled")]
#[test_case(false; "no_min_delegation")]
#[tokio::test]
async fn test_stake_checked_instructions(min_delegation: bool) {
    let mut context = program_test(min_delegation).start_with_context().await;
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

    test_instruction_with_missing_signers(&mut context, &instruction, &vec![&withdrawer_keypair])
        .await;

    // Test AuthorizeChecked with non-signing staker
    let stake =
        create_independent_stake_account(&mut context, &Authorized { staker, withdrawer }, 0).await;
    let instruction =
        ixn::authorize_checked(&stake, &staker, &authorized, StakeAuthorize::Staker, None);

    test_instruction_with_missing_signers(
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

    test_instruction_with_missing_signers(
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

        test_instruction_with_missing_signers(
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

    test_instruction_with_missing_signers(
        &mut context,
        &instruction,
        &vec![&withdrawer_keypair, &custodian_keypair],
    )
    .await;
}

#[test_case(true; "all_enabled")]
#[test_case(false; "no_min_delegation")]
#[tokio::test]
async fn test_stake_initialize(min_delegation: bool) {
    let mut context = program_test(min_delegation).start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let no_signers: &[Keypair] = &[];

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
    process_instruction(&mut context, &instruction, no_signers)
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
    let e = process_instruction(&mut context, &instruction, no_signers)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);

    // not enough balance for rent
    let stake = Pubkey::new_unique();
    let account = SolanaAccount {
        lamports: rent_exempt_reserve / 2,
        data: vec![0; StakeStateV2::size_of()],
        owner: stake_program::id(),
        executable: false,
        rent_epoch: 1000,
    };
    context.set_account(&stake, &account.into());

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let e = process_instruction(&mut context, &instruction, no_signers)
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
        &stake_program::id(),
    );
    process_instruction(&mut context, &instruction, &vec![&stake_keypair])
        .await
        .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let e = process_instruction(&mut context, &instruction, no_signers)
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
        &stake_program::id(),
    );
    process_instruction(&mut context, &instruction, &vec![&stake_keypair])
        .await
        .unwrap();

    let instruction = ixn::initialize(&stake, &authorized, &lockup);
    let e = process_instruction(&mut context, &instruction, no_signers)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);
}

// TODO authorize tests

#[test_case(true; "all_enabled")]
#[test_case(false; "no_min_delegation")]
#[tokio::test]
async fn test_stake_delegate(min_delegation: bool) {
    let mut context = program_test(min_delegation).start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let vote_account2 = Keypair::new();
    create_vote(
        &mut context.banks_client,
        &context.payer,
        &context.last_blockhash,
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

    test_instruction_with_missing_signers(&mut context, &instruction, &vec![&staker_keypair]).await;

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
                deactivation_epoch: std::u64::MAX,
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
    // XXX TODO FIXME pr the fucking stakerror conversion this is driving me insane
    assert_eq!(e, ProgramError::Custom(3));

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
    // XXX TODO FIXME pr the fucking stakerror conversion this is driving me insane
    assert_eq!(e, ProgramError::Custom(3));

    // verify that delegate succeeds to same vote account when stake is deactivating
    refresh_blockhash(&mut context).await;
    let instruction = ixn::delegate_stake(&stake, &staker, &accounts.vote_account.pubkey());
    process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap();

    // verify that deactivation has been cleared
    let (_, stake_data, _) = get_stake_account(&mut context.banks_client, &stake).await;
    assert_eq!(stake_data.unwrap().delegation.deactivation_epoch, u64::MAX);

    // verify that delegate to a different vote account fails if stake is still active
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2.pubkey());
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    // XXX TODO FIXME pr the fucking stakerror conversion this is driving me insane
    assert_eq!(e, ProgramError::Custom(3));

    // delegate still fails after stake is fully activated; redelegate is not supported
    advance_epoch(&mut context).await;
    let instruction = ixn::delegate_stake(&stake, &staker, &vote_account2.pubkey());
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    // XXX TODO FIXME pr the fucking stakerror conversion this is driving me insane
    assert_eq!(e, ProgramError::Custom(3));

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
        owner: stake_program::id(),
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
pub enum SplitSource {
    Uninitialized = 0,
    Initialized,
    Activating,
    Active,
    Deactivating,
    Deactive,
}
impl SplitSource {
    // NOTE the program enforces that a deactive stake adheres to the minimum, albeit spuriously
    // after solana-program/stake-program #1 is addressed, Self::Deactive should move to false
    pub fn minimum_enforced(&self) -> bool {
        match self {
            Self::Activating | Self::Active | Self::Deactivating | Self::Deactive => true,
            Self::Uninitialized | Self::Initialized => false,
        }
    }
}

// XXX TODO FIXME i was today days into this project when i realized i cant test minimum delegation
// because bpf programs cant access features so i just have it hardcoded as off
// consider if we can backdoor edit it in a reasonably clean way or just dont worry about it
// TODO test whole-balance split
#[test_case(SplitSource::Uninitialized, true; "uninitialized::all_enabled")]
#[test_case(SplitSource::Initialized, true; "initialized::all_enabled")]
#[test_case(SplitSource::Activating, true; "activating::all_enabled")]
#[test_case(SplitSource::Active, true; "active::all_enabled")]
#[test_case(SplitSource::Deactivating, true; "deactivating::all_enabled")]
#[test_case(SplitSource::Deactive, true; "deactive::all_enabled")]
#[test_case(SplitSource::Uninitialized, false; "uninitialized::no_min_delegation")]
#[test_case(SplitSource::Initialized, false; "initialized::no_min_delegation")]
#[test_case(SplitSource::Activating, false; "activating::no_min_delegation")]
#[test_case(SplitSource::Active, false; "active::no_min_delegation")]
#[test_case(SplitSource::Deactivating, false; "deactivating::no_min_delegation")]
#[test_case(SplitSource::Deactive, false; "deactive::no_min_delegation")]
#[tokio::test]
async fn test_split(split_source_type: SplitSource, min_delegation: bool) {
    let mut context = program_test(min_delegation).start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();

    let staker = staker_keypair.pubkey();
    let withdrawer = withdrawer_keypair.pubkey();

    let authorized = Authorized { staker, withdrawer };

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let staked_amount = minimum_delegation * 2;

    let uninitialized_source_keypair = Keypair::new();
    let split_source = if split_source_type == SplitSource::Uninitialized {
        let split_source =
            create_blank_stake_account_from_keypair(&mut context, &uninitialized_source_keypair)
                .await;
        transfer(&mut context, &split_source, staked_amount).await;
        split_source
    } else {
        create_independent_stake_account(&mut context, &authorized, staked_amount).await
    };

    if split_source_type >= SplitSource::Activating {
        let instruction =
            ixn::delegate_stake(&split_source, &staker, &accounts.vote_account.pubkey());
        process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap();
    }

    if split_source_type >= SplitSource::Active {
        advance_epoch(&mut context).await;
        assert_eq!(
            get_effective_stake(&mut context.banks_client, &split_source).await,
            staked_amount,
        );
    }

    if split_source_type >= SplitSource::Deactivating {
        let instruction = ixn::deactivate_stake(&split_source, &staker);
        process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap();
    }

    if split_source_type == SplitSource::Deactive {
        advance_epoch(&mut context).await;
        assert_eq!(
            get_effective_stake(&mut context.banks_client, &split_source).await,
            0,
        );
    }

    let split_dest = create_blank_stake_account(&mut context).await;

    let signers = match split_source_type {
        SplitSource::Uninitialized => vec![&uninitialized_source_keypair],
        _ => vec![&staker_keypair],
    };

    // fail, split more than available (even if not active, would kick source out of rent exemption)
    let instruction = &ixn::split(
        &split_source,
        &signers[0].pubkey(),
        staked_amount + 1,
        &split_dest,
    )[2];

    let e = process_instruction(&mut context, &instruction, &signers)
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InsufficientFunds);

    // an active or transitioning stake account cannot have less than the minimum delegation
    // note this is NOT dependent on the minimum delegation feature. there was ALWAYS a minimum. it was one lamport!
    if split_source_type.minimum_enforced() {
        // zero split fails
        let instruction = &ixn::split(&split_source, &signers[0].pubkey(), 0, &split_dest)[2];
        let e = process_instruction(&mut context, &instruction, &signers)
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

        let e = process_instruction(&mut context, &instruction, &signers)
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

        let e = process_instruction(&mut context, &instruction, &signers)
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

    let e = process_instruction(&mut context, &instruction, &signers)
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
    test_instruction_with_missing_signers(&mut context, &instruction, &signers).await;

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
    if split_source_type >= SplitSource::Initialized {
        let (source_meta, source_stake, _) =
            get_stake_account(&mut context.banks_client, &split_source).await;
        let (dest_meta, dest_stake, _) =
            get_stake_account(&mut context.banks_client, &split_dest).await;
        assert_eq!(dest_meta, source_meta);

        // delegations are set properly if activating or active
        if split_source_type >= SplitSource::Activating && split_source_type < SplitSource::Deactive
        {
            assert_eq!(source_stake.unwrap().delegation.stake, staked_amount / 2);
            assert_eq!(dest_stake.unwrap().delegation.stake, staked_amount / 2);
        }
    }

    // nothing has been deactivated if active
    if split_source_type >= SplitSource::Active && split_source_type < SplitSource::Deactive {
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
