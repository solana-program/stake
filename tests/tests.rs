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
            state::{Authorized, Lockup, Meta, Stake, StakeAuthorize, StakeStateV2},
        },
        system_instruction, system_program,
        transaction::{Transaction, TransactionError},
    },
    solana_vote_program::{
        self, vote_instruction,
        vote_state::{VoteInit, VoteState, VoteStateVersions},
    },
    stake_program::processor::Processor,
};

pub const USER_STARTING_LAMPORTS: u64 = 10_000_000_000_000; // 10k sol

pub fn program_test(enable_minimum_delegation: bool) -> ProgramTest {
    let mut program_test = ProgramTest::default();
    // XXX HANA program_test.prefer_bpf(false);

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

pub async fn transfer(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    recipient: &Pubkey,
    amount: u64,
) {
    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &payer.pubkey(),
            recipient,
            amount,
        )],
        Some(&payer.pubkey()),
        &[payer],
        *recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();
}

pub async fn advance_epoch(context: &mut ProgramTestContext) {
    let root_slot = context.banks_client.get_root_slot().await.unwrap();
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    context.warp_to_slot(root_slot + slots_per_epoch).unwrap();
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
    let lamports = get_stake_account_rent(&mut context.banks_client).await;

    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::create_account(
            &context.payer.pubkey(),
            &stake.pubkey(),
            lamports,
            std::mem::size_of::<stake::state::StakeStateV2>() as u64,
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
            if let TransactionError::InstructionError(_, e) = e.unwrap() {
                Err(e.try_into().unwrap())
            } else {
                panic!("couldnt convert {:?} to ProgramError", e,)
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

#[tokio::test]
async fn test_stake_checked_instructions() {
    let mut context = program_test(true).start_with_context().await;
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
