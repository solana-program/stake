#![allow(dead_code)]
#![allow(unused_imports)]

use {
    neostake::omnibus::Processor,
    solana_program_test::*,
    solana_sdk::{
        account::Account as SolanaAccount,
        feature_set::stake_raise_minimum_delegation_to_1_sol,
        hash::Hash,
        native_token::LAMPORTS_PER_SOL,
        program_error::ProgramError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        stake::{
            self,
            state::{Authorized, Lockup, Meta, Stake, StakeStateV2},
        },
        system_instruction, system_program,
        transaction::{Transaction, TransactionError},
    },
    solana_vote_program::{
        self, vote_instruction,
        vote_state::{VoteInit, VoteState, VoteStateVersions},
    },
};

pub const USER_STARTING_LAMPORTS: u64 = 10_000_000_000_000; // 10k sol

pub fn program_test(enable_minimum_delegation: bool) -> ProgramTest {
    let mut program_test = ProgramTest::default();

    program_test.add_program("neostake", neostake::id(), processor!(Processor::process));
    program_test.prefer_bpf(false);

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
    pub alice: Keypair,
    pub bob: Keypair,
    pub alice_stake: Keypair,
    pub bob_stake: Keypair,
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

        transfer(
            &mut context.banks_client,
            &context.payer,
            &context.last_blockhash,
            &self.alice.pubkey(),
            USER_STARTING_LAMPORTS,
        )
        .await;

        transfer(
            &mut context.banks_client,
            &context.payer,
            &context.last_blockhash,
            &self.bob.pubkey(),
            USER_STARTING_LAMPORTS,
        )
        .await;
    }
}

impl Default for Accounts {
    fn default() -> Self {
        let vote_account = Keypair::new();
        let alice = Keypair::new();
        let bob = Keypair::new();

        Self {
            validator: Keypair::new(),
            voter: Keypair::new(),
            withdrawer: Keypair::new(),
            vote_account,
            alice_stake: Keypair::new(),
            bob_stake: Keypair::new(),
            alice,
            bob,
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

#[allow(clippy::too_many_arguments)]
pub async fn create_independent_stake_account(
    banks_client: &mut BanksClient,
    fee_payer: &Keypair,
    rent_payer: &Keypair,
    recent_blockhash: &Hash,
    stake: &Keypair,
    authorized: &stake::state::Authorized,
    lockup: &stake::state::Lockup,
    stake_amount: u64,
) -> u64 {
    let lamports = get_stake_account_rent(banks_client).await + stake_amount;
    let mut instructions = vec![
        system_instruction::create_account(
            &rent_payer.pubkey(),
            &stake.pubkey(),
            lamports,
            StakeStateV2::size_of() as u64,
            &neostake::id(),
        ),
        stake::instruction::initialize(&stake.pubkey(), authorized, lockup),
    ];
    instructions[1].program_id = neostake::id();

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&fee_payer.pubkey()),
        &[fee_payer, rent_payer, stake],
        *recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();

    lamports
}

pub async fn delegate_stake_account(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    stake: &Pubkey,
    authorized: &Keypair,
    vote: &Pubkey,
) {
    let mut instruction = stake::instruction::delegate_stake(stake, &authorized.pubkey(), vote);
    instruction.program_id = neostake::id();

    let mut transaction = Transaction::new_with_payer(&[instruction], Some(&payer.pubkey()));
    transaction.sign(&[payer, authorized], *recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();
}

#[tokio::test]
async fn hana_test() {
    let mut context = program_test(true).start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    create_independent_stake_account(
        &mut context.banks_client,
        &context.payer,
        &accounts.alice,
        &context.last_blockhash,
        &accounts.alice_stake,
        &Authorized::auto(&accounts.alice.pubkey()),
        &Lockup::default(),
        LAMPORTS_PER_SOL,
    )
    .await;

    let stake_info =
        get_stake_account(&mut context.banks_client, &accounts.alice_stake.pubkey()).await;
    println!(
        "HANA {} after init: {:?}",
        accounts.alice_stake.pubkey(),
        stake_info
    );

    delegate_stake_account(
        &mut context.banks_client,
        &context.payer,
        &context.last_blockhash,
        &accounts.alice_stake.pubkey(),
        &accounts.alice,
        &accounts.vote_account.pubkey(),
    )
    .await;

    let stake_info =
        get_stake_account(&mut context.banks_client, &accounts.alice_stake.pubkey()).await;
    println!(
        "HANA {} after delegate: {:?}",
        accounts.alice_stake.pubkey(),
        stake_info
    );
}
