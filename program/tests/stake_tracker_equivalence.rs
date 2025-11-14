#![allow(clippy::arithmetic_side_effects)]

//! Equivalence tests proving StakeTracker (Mollusk) matches BanksClient (solana-program-test)
//!
//! These tests run identical stake operations through both implementations and compare results
//! to ensure 1:1 behavioral equivalence in stake history tracking.
use {
    mollusk_svm::Mollusk,
    solana_account::{AccountSharedData, ReadableAccount, WritableAccount},
    solana_clock::Clock,
    solana_keypair::Keypair,
    solana_program_test::{ProgramTest, ProgramTestContext},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_stake_interface::{
        instruction as ixn,
        stake_history::StakeHistory,
        state::{Authorized, Lockup, StakeStateV2},
    },
    solana_stake_program::id,
    solana_system_interface::instruction as system_instruction,
    solana_transaction::Transaction,
    test_case::test_case,
};

mod helpers;
use helpers::{
    stake_tracker::{MolluskStakeExt, StakeTracker},
    utils::{add_sysvars, create_vote_account, STAKE_RENT_EXEMPTION},
};

// Constants for testing
const MINIMUM_DELEGATION: u64 = 1;

/// Dual context holding both BanksClient and Mollusk paths
struct DualContext {
    // BanksClient path
    program_test_ctx: ProgramTestContext,

    // Mollusk path
    mollusk: Mollusk,
    tracker: StakeTracker,

    // Shared test data
    vote_account: Pubkey,
    vote_account_data: AccountSharedData,
    background_stake: u64,
}

impl DualContext {
    /// Create both contexts with matching initial state
    async fn new() -> Self {
        // Initialize program test (BanksClient path)
        let mut program_test = ProgramTest::default();
        program_test.prefer_bpf(true);
        program_test.add_upgradeable_program_to_genesis("solana_stake_program", &id());
        let mut program_test_ctx = program_test.start_with_context().await;

        // Warp to first normal slot on Banks
        let slot = program_test_ctx
            .genesis_config()
            .epoch_schedule
            .first_normal_slot
            + 1;
        program_test_ctx.warp_to_slot(slot).unwrap();

        // Initialize Mollusk and sync to the same epoch as Banks
        let mut mollusk = Mollusk::new(&id(), "solana_stake_program");
        // Banks and Mollusk have different epoch schedules, so we need to get to same epoch
        let banks_clock = program_test_ctx
            .banks_client
            .get_sysvar::<Clock>()
            .await
            .unwrap();
        let banks_epoch = banks_clock.epoch;

        // Warp Mollusk to the same epoch by calculating the corresponding slot
        let mollusk_slot_for_epoch = mollusk
            .sysvars
            .epoch_schedule
            .get_first_slot_in_epoch(banks_epoch);
        mollusk.warp_to_slot(mollusk_slot_for_epoch);

        // Extract BanksClient's actual background stake from its genesis stake history
        // This ensures Mollusk uses the same background stake for identical history
        let banks_stake_history = program_test_ctx
            .banks_client
            .get_sysvar::<StakeHistory>()
            .await
            .unwrap();

        let epoch_0_entry = banks_stake_history.get(0).cloned().unwrap_or_default();
        let background_stake = epoch_0_entry.effective;

        let tracker = StakeTracker::with_background_stake(background_stake);

        // Initialize Mollusk's stake history to match BanksClient
        // Banks may not have history for all intermediate epochs when warped directly,
        // so we'll generate them using the tracker (which only has background stake initially)
        for epoch in 0..banks_epoch {
            if let Some(entry) = banks_stake_history.get(epoch).cloned() {
                // Use Banks' actual entry if it exists
                mollusk.sysvars.stake_history.add(epoch, entry);
            } else {
                // Generate entry with just background stake for missing epochs
                // This matches what would happen if we had advanced through these epochs naturally
                mollusk
                    .sysvars
                    .stake_history
                    .add(epoch, epoch_0_entry.clone());
            }
        }

        // Create shared vote account
        let vote_account = Pubkey::new_unique();
        let vote_account_data = create_vote_account();

        // Add vote account to BanksClient (clone to keep original)
        program_test_ctx.set_account(&vote_account, &vote_account_data.clone());

        Self {
            program_test_ctx,
            mollusk,
            tracker,
            vote_account,
            vote_account_data,
            background_stake,
        }
    }

    /// Create a blank stake account on both paths
    async fn create_blank_stake_account(&mut self) -> Pubkey {
        let stake_keypair = Keypair::new();
        let stake = stake_keypair.pubkey();

        // BanksClient path
        let transaction = Transaction::new_signed_with_payer(
            &[system_instruction::create_account(
                &self.program_test_ctx.payer.pubkey(),
                &stake,
                STAKE_RENT_EXEMPTION,
                StakeStateV2::size_of() as u64,
                &id(),
            )],
            Some(&self.program_test_ctx.payer.pubkey()),
            &[&self.program_test_ctx.payer, &stake_keypair],
            self.program_test_ctx.last_blockhash,
        );
        self.program_test_ctx
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap();

        // Mollusk path - just track that we'll add it when needed
        // (Mollusk accounts are passed per-instruction, not stored globally)

        stake
    }

    /// Initialize a stake account on both paths
    async fn initialize_stake_account(
        &mut self,
        stake: &Pubkey,
        authorized: &Authorized,
        lockup: &Lockup,
    ) -> AccountSharedData {
        let instruction = ixn::initialize(stake, authorized, lockup);

        // BanksClient path
        let transaction = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&self.program_test_ctx.payer.pubkey()),
            &[&self.program_test_ctx.payer],
            self.program_test_ctx.last_blockhash,
        );
        self.program_test_ctx
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap();

        // Get account from BanksClient
        let banks_account = self
            .program_test_ctx
            .banks_client
            .get_account(*stake)
            .await
            .unwrap()
            .unwrap();

        // Mollusk path - create matching account
        let mut mollusk_account =
            AccountSharedData::new(STAKE_RENT_EXEMPTION, StakeStateV2::size_of(), &id());

        let accounts = vec![(*stake, mollusk_account.clone())];
        let accounts_with_sysvars = add_sysvars(&self.mollusk, &instruction, accounts);
        let result = self
            .mollusk
            .process_instruction(&instruction, &accounts_with_sysvars);
        assert!(result.program_result.is_ok());
        mollusk_account = result.resulting_accounts[0].1.clone().into();

        // Verify accounts match
        assert_eq!(banks_account.data, mollusk_account.data());
        assert_eq!(banks_account.lamports, mollusk_account.lamports());

        mollusk_account
    }

    /// Delegate stake on both paths to a specific vote account
    async fn delegate_stake_to(
        &mut self,
        stake: &Pubkey,
        stake_account: &mut AccountSharedData,
        staker_keypair: &Keypair,
        vote_account: &Pubkey,
        vote_account_data: &AccountSharedData,
    ) {
        let instruction = ixn::delegate_stake(stake, &staker_keypair.pubkey(), vote_account);

        // BanksClient path
        let transaction = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&self.program_test_ctx.payer.pubkey()),
            &[&self.program_test_ctx.payer, staker_keypair],
            self.program_test_ctx.last_blockhash,
        );
        self.program_test_ctx
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap();

        // Mollusk path
        let accounts = vec![
            (*stake, stake_account.clone()),
            (*vote_account, vote_account_data.clone()),
        ];
        let accounts_with_sysvars = add_sysvars(&self.mollusk, &instruction, accounts);
        let result = self
            .mollusk
            .process_instruction(&instruction, &accounts_with_sysvars);
        assert!(result.program_result.is_ok());
        *stake_account = result.resulting_accounts[0].1.clone().into();

        // Track delegation in Mollusk tracker
        let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();
        if let StakeStateV2::Stake(_, stake_data, _) = stake_state {
            self.tracker.track_delegation(
                stake,
                stake_data.delegation.stake,
                stake_data.delegation.activation_epoch,
                vote_account,
            );
        }
    }

    /// Delegate stake on both paths (uses default vote account)
    async fn delegate_stake(
        &mut self,
        stake: &Pubkey,
        stake_account: &mut AccountSharedData,
        staker_keypair: &Keypair,
    ) {
        let vote_account = self.vote_account;
        let vote_account_data = self.vote_account_data.clone();
        self.delegate_stake_to(
            stake,
            stake_account,
            staker_keypair,
            &vote_account,
            &vote_account_data,
        )
        .await;
    }

    /// Deactivate stake on both paths
    async fn deactivate_stake(
        &mut self,
        stake: &Pubkey,
        stake_account: &mut AccountSharedData,
        staker_keypair: &Keypair,
    ) {
        let instruction = ixn::deactivate_stake(stake, &staker_keypair.pubkey());

        // BanksClient path
        let transaction = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&self.program_test_ctx.payer.pubkey()),
            &[&self.program_test_ctx.payer, staker_keypair],
            self.program_test_ctx.last_blockhash,
        );
        self.program_test_ctx
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap();

        // Mollusk path
        let accounts = vec![(*stake, stake_account.clone())];
        let accounts_with_sysvars = add_sysvars(&self.mollusk, &instruction, accounts);
        let result = self
            .mollusk
            .process_instruction(&instruction, &accounts_with_sysvars);
        assert!(result.program_result.is_ok());
        *stake_account = result.resulting_accounts[0].1.clone().into();

        // Track deactivation
        let deactivation_epoch = self.mollusk.sysvars.clock.epoch;
        self.tracker.track_deactivation(stake, deactivation_epoch);
    }

    /// Advance epoch on both paths with default new_rate_activation_epoch (Some(0))
    async fn advance_epoch(&mut self) {
        self.advance_epoch_with_rate(Some(0)).await;
    }

    /// Advance epoch on both paths with custom new_rate_activation_epoch
    /// Pass None to use old warmup rate behavior
    async fn advance_epoch_with_rate(&mut self, new_rate_activation_epoch: Option<u64>) {
        // Refresh blockhash for BanksClient by advancing slot slightly first
        let current_slot = self
            .program_test_ctx
            .banks_client
            .get_root_slot()
            .await
            .unwrap();
        self.program_test_ctx
            .warp_to_slot(current_slot + 1)
            .unwrap();
        self.program_test_ctx.last_blockhash = self
            .program_test_ctx
            .banks_client
            .get_latest_blockhash()
            .await
            .unwrap();

        // BanksClient path - advance epoch
        let root_slot = self
            .program_test_ctx
            .banks_client
            .get_root_slot()
            .await
            .unwrap();
        let slots_per_epoch = self
            .program_test_ctx
            .genesis_config()
            .epoch_schedule
            .slots_per_epoch;
        self.program_test_ctx
            .warp_to_slot(root_slot + slots_per_epoch)
            .unwrap();

        // Mollusk path - advance epoch with stake tracking
        let current_slot = self.mollusk.sysvars.clock.slot;
        let mollusk_slots_per_epoch = self.mollusk.sysvars.epoch_schedule.slots_per_epoch;
        let target_slot = current_slot + mollusk_slots_per_epoch;
        self.mollusk.warp_to_slot_with_stake_tracking(
            &self.tracker,
            target_slot,
            new_rate_activation_epoch,
        );
    }

    /// Fast-forward multiple epochs without full validation (for performance)
    /// This advances both paths in bulk to avoid excessive async operations
    #[allow(dead_code)]
    async fn advance_epochs_fast(&mut self, num_epochs: u64) {
        if num_epochs == 0 {
            return;
        }

        // BanksClient: advance in one big jump
        let root_slot = self
            .program_test_ctx
            .banks_client
            .get_root_slot()
            .await
            .unwrap();
        let slots_per_epoch = self
            .program_test_ctx
            .genesis_config()
            .epoch_schedule
            .slots_per_epoch;
        let target_slot = root_slot + (slots_per_epoch * num_epochs);
        self.program_test_ctx.warp_to_slot(target_slot).unwrap();
        self.program_test_ctx.last_blockhash = self
            .program_test_ctx
            .banks_client
            .get_latest_blockhash()
            .await
            .unwrap();

        // Mollusk: advance in one big jump
        for _ in 0..num_epochs {
            let current_slot = self.mollusk.sysvars.clock.slot;
            let mollusk_slots_per_epoch = self.mollusk.sysvars.epoch_schedule.slots_per_epoch;
            let target_slot = current_slot + mollusk_slots_per_epoch;
            self.mollusk
                .warp_to_slot_with_stake_tracking(&self.tracker, target_slot, Some(0));
        }
    }

    /// Get stake history from BanksClient
    async fn get_banks_stake_history(&mut self) -> StakeHistory {
        self.program_test_ctx
            .banks_client
            .get_sysvar::<StakeHistory>()
            .await
            .unwrap()
    }

    /// Get stake history from Mollusk
    fn get_mollusk_stake_history(&self) -> &StakeHistory {
        &self.mollusk.sysvars.stake_history
    }

    /// Get effective stake from BanksClient
    async fn get_banks_effective_stake(&mut self, stake: &Pubkey) -> u64 {
        let clock = self
            .program_test_ctx
            .banks_client
            .get_sysvar::<Clock>()
            .await
            .unwrap();
        let stake_history = self.get_banks_stake_history().await;
        let account = self
            .program_test_ctx
            .banks_client
            .get_account(*stake)
            .await
            .unwrap()
            .unwrap();

        match bincode::deserialize::<StakeStateV2>(&account.data).unwrap() {
            StakeStateV2::Stake(_, stake_data, _) => {
                stake_data
                    .delegation
                    .stake_activating_and_deactivating(clock.epoch, &stake_history, Some(0))
                    .effective
            }
            _ => 0,
        }
    }

    /// Get effective stake from Mollusk
    fn get_mollusk_effective_stake(&self, stake_account: &AccountSharedData) -> u64 {
        let clock = &self.mollusk.sysvars.clock;
        let stake_history = &self.mollusk.sysvars.stake_history;

        match bincode::deserialize::<StakeStateV2>(stake_account.data()).unwrap() {
            StakeStateV2::Stake(_, stake_data, _) => {
                stake_data
                    .delegation
                    .stake_activating_and_deactivating(clock.epoch, stake_history, Some(0))
                    .effective
            }
            _ => 0,
        }
    }

    /// Compare stake history entries between both implementations
    /// Verifies all components: effective, activating, and deactivating
    async fn compare_stake_history(&mut self, epoch: u64) {
        let banks_history = self
            .program_test_ctx
            .banks_client
            .get_sysvar::<StakeHistory>()
            .await
            .unwrap();
        let mollusk_history = &self.mollusk.sysvars.stake_history;

        let banks_entry = banks_history.get(epoch);
        let mollusk_entry = mollusk_history.get(epoch);

        assert_eq!(
            banks_entry, mollusk_entry,
            "Stake history mismatch at epoch {}: BanksClient={:?}, Mollusk={:?}",
            epoch, banks_entry, mollusk_entry
        );
    }

    /// Verify background stake is preserved in stake history across implementations
    async fn verify_background_stake_preservation(&mut self, epoch: u64, expected_background: u64) {
        let banks_history = self
            .program_test_ctx
            .banks_client
            .get_sysvar::<StakeHistory>()
            .await
            .unwrap();
        let mollusk_history = &self.mollusk.sysvars.stake_history;

        if let Some(banks_entry) = banks_history.get(epoch) {
            assert!(
                banks_entry.effective >= expected_background,
                "Epoch {}: Banks effective stake {} should include background {}",
                epoch,
                banks_entry.effective,
                expected_background
            );
        }

        if let Some(mollusk_entry) = mollusk_history.get(epoch) {
            assert!(
                mollusk_entry.effective >= expected_background,
                "Epoch {}: Mollusk effective stake {} should include background {}",
                epoch,
                mollusk_entry.effective,
                expected_background
            );
        }
    }

    /// Compare account state between both paths
    async fn compare_account_state(&mut self, stake: &Pubkey, mollusk_account: &AccountSharedData) {
        let banks_account = self
            .program_test_ctx
            .banks_client
            .get_account(*stake)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            banks_account.lamports,
            mollusk_account.lamports(),
            "Lamports mismatch"
        );

        let banks_state: StakeStateV2 = bincode::deserialize(&banks_account.data).unwrap();
        let mollusk_state: StakeStateV2 = bincode::deserialize(mollusk_account.data()).unwrap();

        match (banks_state, mollusk_state) {
            (StakeStateV2::Stake(b_meta, b_stake, _), StakeStateV2::Stake(m_meta, m_stake, _)) => {
                assert_eq!(b_meta.authorized, m_meta.authorized);
                assert_eq!(b_meta.lockup, m_meta.lockup);
                assert_eq!(b_stake.delegation.stake, m_stake.delegation.stake);
                assert_eq!(
                    b_stake.delegation.activation_epoch,
                    m_stake.delegation.activation_epoch
                );
                assert_eq!(
                    b_stake.delegation.deactivation_epoch,
                    m_stake.delegation.deactivation_epoch
                );
            }
            (StakeStateV2::Initialized(b_meta), StakeStateV2::Initialized(m_meta)) => {
                assert_eq!(b_meta.authorized, m_meta.authorized);
                assert_eq!(b_meta.lockup, m_meta.lockup);
            }
            _ => {
                panic!(
                    "State type mismatch: banks={:?}, mollusk={:?}",
                    banks_state, mollusk_state
                );
            }
        }
    }

    /// Advance one epoch and compare stake account state between implementations
    async fn advance_and_compare_stake(
        &mut self,
        stake: &Pubkey,
        stake_account: &AccountSharedData,
    ) {
        self.advance_and_compare_stake_with_rate(stake, stake_account, Some(0))
            .await;
    }

    /// Advance one epoch with custom warmup rate and compare stake account state
    async fn advance_and_compare_stake_with_rate(
        &mut self,
        stake: &Pubkey,
        stake_account: &AccountSharedData,
        new_rate_activation_epoch: Option<u64>,
    ) {
        self.advance_epoch_with_rate(new_rate_activation_epoch)
            .await;
        let epoch = self.mollusk.sysvars.clock.epoch;
        self.compare_stake_history(epoch - 1).await;

        let banks_effective = self.get_banks_effective_stake(stake).await;
        let mollusk_effective = self.get_mollusk_effective_stake(stake_account);
        assert_eq!(
            banks_effective, mollusk_effective,
            "Epoch {}: effective stake mismatch",
            epoch
        );
        self.compare_account_state(stake, stake_account).await;
    }

    /// Advance epochs until stake is fully activated (effective == delegated amount)
    /// Returns number of epochs advanced. Panics if not activated within max_epochs.
    async fn advance_until_fully_activated(
        &mut self,
        stake: &Pubkey,
        stake_account: &AccountSharedData,
        max_epochs: u64,
    ) -> u64 {
        let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();
        let target_amount = match stake_state {
            StakeStateV2::Stake(_, stake_data, _) => stake_data.delegation.stake,
            _ => panic!("Stake account not in delegated state"),
        };

        let mut epochs_advanced = 0;
        loop {
            self.advance_and_compare_stake(stake, stake_account).await;
            epochs_advanced += 1;

            let banks_effective = self.get_banks_effective_stake(stake).await;
            if banks_effective == target_amount {
                return epochs_advanced;
            }

            assert!(
                epochs_advanced < max_epochs,
                "Stake did not fully activate within {} epochs",
                max_epochs
            );
        }
    }

    /// Advance epochs until stake is fully deactivated (effective == 0)
    /// Returns number of epochs advanced. Panics if not deactivated within max_epochs.
    async fn advance_until_fully_deactivated(
        &mut self,
        stake: &Pubkey,
        stake_account: &AccountSharedData,
        max_epochs: u64,
    ) -> u64 {
        let mut epochs_advanced = 0;
        loop {
            self.advance_and_compare_stake(stake, stake_account).await;
            epochs_advanced += 1;

            let banks_effective = self.get_banks_effective_stake(stake).await;
            if banks_effective == 0 {
                return epochs_advanced;
            }

            assert!(
                epochs_advanced < max_epochs,
                "Stake did not fully deactivate within {} epochs",
                max_epochs
            );
        }
    }

    /// Advance one epoch and compare multiple stake accounts between implementations
    async fn advance_and_compare_stakes(&mut self, stakes: &[(&Pubkey, &AccountSharedData)]) {
        self.advance_and_compare_stakes_with_rate(stakes, Some(0))
            .await;
    }

    /// Advance one epoch with custom warmup rate and compare multiple stake accounts
    async fn advance_and_compare_stakes_with_rate(
        &mut self,
        stakes: &[(&Pubkey, &AccountSharedData)],
        new_rate_activation_epoch: Option<u64>,
    ) {
        self.advance_epoch_with_rate(new_rate_activation_epoch)
            .await;
        let epoch = self.mollusk.sysvars.clock.epoch;
        self.compare_stake_history(epoch - 1).await;

        for (stake, stake_account) in stakes {
            let banks_effective = self.get_banks_effective_stake(stake).await;
            let mollusk_effective = self.get_mollusk_effective_stake(stake_account);
            assert_eq!(
                banks_effective, mollusk_effective,
                "Epoch {}: stake {} mismatch",
                epoch, stake
            );
            self.compare_account_state(stake, stake_account).await;
        }
    }

    /// Fund a stake account on BanksClient side
    async fn fund_stake_account(&mut self, stake: &Pubkey, amount: u64) {
        let fund_ix =
            system_instruction::transfer(&self.program_test_ctx.payer.pubkey(), stake, amount);
        let transaction = Transaction::new_signed_with_payer(
            &[fund_ix],
            Some(&self.program_test_ctx.payer.pubkey()),
            &[&self.program_test_ctx.payer],
            self.program_test_ctx.last_blockhash,
        );
        self.program_test_ctx
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap();
    }

    /// Create, initialize, and fund a stake account
    /// Returns (stake pubkey, stake account, staker keypair)
    async fn create_and_fund_stake(
        &mut self,
        staked_amount: u64,
        lockup: &Lockup,
    ) -> (Pubkey, AccountSharedData, Keypair) {
        let stake = self.create_blank_stake_account().await;
        let staker = Keypair::new();
        let authorized = Authorized {
            staker: staker.pubkey(),
            withdrawer: Pubkey::new_unique(),
        };

        let mut stake_account = self
            .initialize_stake_account(&stake, &authorized, lockup)
            .await;

        stake_account.set_lamports(stake_account.lamports() + staked_amount);
        self.fund_stake_account(&stake, staked_amount).await;

        (stake, stake_account, staker)
    }

    /// Create and register a new vote account on BanksClient
    /// Returns (vote account pubkey, vote account data)
    fn create_vote_account(&mut self) -> (Pubkey, AccountSharedData) {
        let vote_account = Pubkey::new_unique();
        let vote_account_data = create_vote_account();
        self.program_test_ctx
            .set_account(&vote_account, &vote_account_data.clone());
        (vote_account, vote_account_data)
    }

    /// Create multiple stakes with minimal funding (for testing multiple simultaneous operations)
    /// Returns (stakes, stake_accounts, stakers)
    async fn create_multiple_stakes(
        &mut self,
        count: usize,
        staked_amount: u64,
    ) -> (Vec<Pubkey>, Vec<AccountSharedData>, Vec<Keypair>) {
        let mut stakes = Vec::new();
        let mut stake_accounts = Vec::new();
        let mut stakers = Vec::new();

        for _ in 0..count {
            let stake = self.create_blank_stake_account().await;
            let staker = Keypair::new();
            let authorized = Authorized {
                staker: staker.pubkey(),
                withdrawer: Pubkey::new_unique(),
            };

            let mut stake_account = self
                .initialize_stake_account(&stake, &authorized, &Lockup::default())
                .await;

            stake_account.set_lamports(stake_account.lamports() + staked_amount);
            self.fund_stake_account(&stake, staked_amount).await;

            stakes.push(stake);
            stake_accounts.push(stake_account);
            stakers.push(staker);
        }

        (stakes, stake_accounts, stakers)
    }

    /// Verify background stake preservation across a range of epochs
    async fn verify_background_stake_across_epochs(
        &mut self,
        num_epochs: u64,
        expected_background: u64,
    ) {
        for epoch_offset in 0..num_epochs {
            let epoch = self.mollusk.sysvars.clock.epoch - num_epochs + epoch_offset;
            self.verify_background_stake_preservation(epoch, expected_background)
                .await;
        }
    }
}

// Test single delegation activation over various epoch counts
//
// Uses bulk fast-forward + sampling for large epoch counts to prove equivalence
// without excessive runtime.
#[test_case(5, 1 ; "five_epochs")]
#[test_case(10, 1 ; "ten_epochs")]
#[test_case(20, 1 ; "twenty_epochs")]
#[test_case(50, 1 ; "fifty_epochs")]
#[test_case(182, 2 ; "one_year")]
#[test_case(515, 5 ; "beyond_depth")]
#[test_case(1820, 50 ; "ten_years")]
#[tokio::test]
async fn test_single_delegation_activation(ending_epoch: u64, sample_rate: u64) {
    let mut ctx = DualContext::new().await;

    // Create, initialize, and fund stake account
    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    // Delegate stake
    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    const BOUNDARY: u64 = 10;

    for i in 0..ending_epoch {
        let is_warmup_boundary = i < BOUNDARY;
        let is_end_boundary = i >= ending_epoch.saturating_sub(BOUNDARY);
        let is_sample = i % sample_rate == 0;

        if is_warmup_boundary || is_end_boundary || is_sample {
            // Full validation at boundaries and sample points
            ctx.advance_and_compare_stake(&stake, &stake_account).await;

            // Periodic background stake checks
            let check_frequency = if ending_epoch <= 50 { 10 } else { 50 };
            if i % check_frequency == 0 || i == ending_epoch - 1 {
                let epoch = ctx.mollusk.sysvars.clock.epoch;
                ctx.verify_background_stake_preservation(epoch - 1, ctx.background_stake)
                    .await;
            }
        } else {
            ctx.advance_epoch().await;
        }
    }
}

// Test deactivation at different points in lifecycle: immediately, during warmup, or after activation
#[test_case(0 ; "immediate_same_epoch")]
#[test_case(1 ; "early_warmup")]
#[test_case(2 ; "mid_warmup")]
#[test_case(3 ; "late_warmup")]
#[test_case(5 ; "after_activation")]
#[tokio::test]
async fn test_deactivation_timing(epochs_before_deactivate: u64) {
    let mut ctx = DualContext::new().await;

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    for _ in 0..epochs_before_deactivate {
        ctx.advance_and_compare_stake(&stake, &stake_account).await;
    }

    ctx.deactivate_stake(&stake, &mut stake_account, &staker)
        .await;

    ctx.advance_until_fully_deactivated(&stake, &stake_account, 20)
        .await;
}

// Test various stake amounts through full lifecycle: activation + deactivation
// Verifies warmup/cooldown rates work correctly for minimum, small, and large stakes
#[test_case(MINIMUM_DELEGATION ; "minimum_delegation")]
#[test_case(MINIMUM_DELEGATION * 100 ; "small_amount")]
#[test_case(250_000 * 1_000_000_000 ; "large_amount")]
#[tokio::test]
async fn test_stake_amounts_full_lifecycle(staked_amount: u64) {
    let mut ctx = DualContext::new().await;

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(staked_amount, &Lockup::default())
        .await;

    // Activate
    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    ctx.advance_until_fully_activated(&stake, &stake_account, 50)
        .await;

    // Deactivate
    ctx.deactivate_stake(&stake, &mut stake_account, &staker)
        .await;

    ctx.advance_until_fully_deactivated(&stake, &stake_account, 50)
        .await;
}

// Test multiple stakes delegating simultaneously, optionally to multiple vote accounts
#[test_case(5, 1 ; "five_stakes_one_vote")]
#[test_case(10, 1 ; "ten_stakes_one_vote")]
#[test_case(20, 1 ; "twenty_stakes_one_vote")]
#[test_case(4, 2 ; "four_stakes_two_votes")]
#[tokio::test]
async fn test_multiple_simultaneous_delegations(num_stakes: usize, num_vote_accounts: usize) {
    let mut ctx = DualContext::new().await;

    // Create additional vote accounts if needed
    let mut vote_accounts = vec![(ctx.vote_account, ctx.vote_account_data.clone())];
    for _ in 1..num_vote_accounts {
        vote_accounts.push(ctx.create_vote_account());
    }

    let (stakes, mut stake_accounts, stakers) = ctx
        .create_multiple_stakes(num_stakes, MINIMUM_DELEGATION)
        .await;

    // Delegate all stakes (alternating vote accounts if multiple)
    for i in 0..num_stakes {
        let (vote_account, vote_account_data) = &vote_accounts[i % num_vote_accounts];
        ctx.delegate_stake_to(
            &stakes[i],
            &mut stake_accounts[i],
            &stakers[i],
            vote_account,
            vote_account_data,
        )
        .await;
    }

    for _ in 0..10 {
        let stake_refs: Vec<_> = stakes.iter().zip(stake_accounts.iter()).collect();
        ctx.advance_and_compare_stakes(&stake_refs).await;
    }
}

#[tokio::test]
async fn test_concurrent_activation_and_deactivation() {
    let mut ctx = DualContext::new().await;

    let (stake_a, mut stake_account_a, staker_a) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    let (stake_b, mut stake_account_b, staker_b) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    ctx.delegate_stake(&stake_b, &mut stake_account_b, &staker_b)
        .await;
    ctx.advance_epoch().await;

    ctx.deactivate_stake(&stake_b, &mut stake_account_b, &staker_b)
        .await;
    ctx.delegate_stake(&stake_a, &mut stake_account_a, &staker_a)
        .await;

    let epoch = ctx.mollusk.sysvars.clock.epoch;

    for _ in 0..5 {
        ctx.advance_epoch().await;
        let current_epoch = ctx.mollusk.sysvars.clock.epoch;
        ctx.compare_stake_history(current_epoch - 1).await;

        let banks_effective_a = ctx.get_banks_effective_stake(&stake_a).await;
        let mollusk_effective_a = ctx.get_mollusk_effective_stake(&stake_account_a);
        assert_eq!(banks_effective_a, mollusk_effective_a);
        ctx.compare_account_state(&stake_a, &stake_account_a).await;

        let banks_effective_b = ctx.get_banks_effective_stake(&stake_b).await;
        let mollusk_effective_b = ctx.get_mollusk_effective_stake(&stake_account_b);
        assert_eq!(banks_effective_b, mollusk_effective_b);
        ctx.compare_account_state(&stake_b, &stake_account_b).await;
    }

    let banks_history = ctx.get_banks_stake_history().await;
    let mollusk_history = ctx.get_mollusk_stake_history();

    let banks_entry = banks_history.get(epoch).unwrap();
    let mollusk_entry = mollusk_history.get(epoch).unwrap();

    assert_eq!(banks_entry.activating, mollusk_entry.activating);
    assert_eq!(banks_entry.deactivating, mollusk_entry.deactivating);
}

#[tokio::test]
async fn test_reactivation_after_deactivation() {
    let mut ctx = DualContext::new().await;

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    for _ in 0..2 {
        ctx.advance_and_compare_stake(&stake, &stake_account).await;
    }

    ctx.deactivate_stake(&stake, &mut stake_account, &staker)
        .await;

    ctx.advance_until_fully_deactivated(&stake, &stake_account, 20)
        .await;

    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    ctx.advance_until_fully_activated(&stake, &stake_account, 20)
        .await;
}

#[tokio::test]
async fn test_staggered_delegations_over_epochs() {
    let mut ctx = DualContext::new().await;

    let (stakes, mut stake_accounts, stakers) =
        ctx.create_multiple_stakes(5, MINIMUM_DELEGATION).await;

    // Stagger delegations across epochs (0, 5, 10, 15, 20)
    for (idx, target_epoch) in [0, 5, 10, 15, 20].iter().enumerate() {
        // Advance to target epoch
        while ctx.mollusk.sysvars.clock.epoch < *target_epoch {
            ctx.advance_epoch().await;
        }

        // Delegate at this epoch
        ctx.delegate_stake(&stakes[idx], &mut stake_accounts[idx], &stakers[idx])
            .await;
    }

    // Advance and compare until epoch 30
    while ctx.mollusk.sysvars.clock.epoch < 30 {
        let stake_refs: Vec<_> = stakes.iter().zip(stake_accounts.iter()).collect();
        ctx.advance_and_compare_stakes(&stake_refs).await;
    }
}

#[tokio::test]
async fn test_mixed_lifecycle_stress() {
    let mut ctx = DualContext::new().await;

    let (stakes, mut stake_accounts, stakers) =
        ctx.create_multiple_stakes(20, MINIMUM_DELEGATION).await;

    // Create various lifecycle states:
    // 5 activating (delegate in epoch 0)
    for i in 0..5 {
        ctx.delegate_stake(&stakes[i], &mut stake_accounts[i], &stakers[i])
            .await;
    }

    ctx.advance_epoch().await; // Epoch 1

    // 5 more activating (delegate in epoch 1)
    for i in 5..10 {
        ctx.delegate_stake(&stakes[i], &mut stake_accounts[i], &stakers[i])
            .await;
    }

    ctx.advance_epoch().await; // Epoch 2

    // 5 deactivating
    for i in 0..5 {
        ctx.deactivate_stake(&stakes[i], &mut stake_accounts[i], &stakers[i])
            .await;
    }

    // Advance and observe mixed states
    for _ in 0..10 {
        let stake_refs: Vec<_> = stakes.iter().zip(stake_accounts.iter()).collect();
        ctx.advance_and_compare_stakes(&stake_refs).await;
    }
}

// Test repeated cycles of delegation and deactivation with full transitions
#[test_case(2 ; "two_cycles")]
#[test_case(3 ; "three_cycles")]
#[tokio::test]
async fn test_repeated_delegation_cycles(num_cycles: usize) {
    let mut ctx = DualContext::new().await;

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    for _ in 0..num_cycles {
        ctx.delegate_stake(&stake, &mut stake_account, &staker)
            .await;
        ctx.advance_until_fully_activated(&stake, &stake_account, 20)
            .await;

        ctx.deactivate_stake(&stake, &mut stake_account, &staker)
            .await;
        ctx.advance_until_fully_deactivated(&stake, &stake_account, 20)
            .await;
    }
}

// Test re-delegating to a different validator at various points during deactivation
//
// RedelegationTiming:
// - AfterFullActivation: Deactivate after full activation, then redelegate after full deactivation
// - DuringWarmup: Deactivate during warmup (after 2 epochs), then redelegate after full deactivation
// - DuringCooldown: Deactivate after full activation, then redelegate during cooldown (after 2 epochs)
#[test_case("AfterFullActivation" ; "after_full_deactivation")]
#[test_case("DuringWarmup" ; "after_deactivation_during_warmup")]
#[test_case("DuringCooldown" ; "during_deactivation_cooldown")]
#[tokio::test]
async fn test_redelegation_to_different_validator(timing: &str) {
    let mut ctx = DualContext::new().await;

    let (vote_account_b, vote_account_b_data) = ctx.create_vote_account();

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    // Initial delegation
    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    // Setup based on timing
    match timing {
        "AfterFullActivation" => {
            ctx.advance_until_fully_activated(&stake, &stake_account, 20)
                .await;
            ctx.deactivate_stake(&stake, &mut stake_account, &staker)
                .await;
            ctx.advance_until_fully_deactivated(&stake, &stake_account, 20)
                .await;
        }
        "DuringWarmup" => {
            ctx.advance_and_compare_stake(&stake, &stake_account).await;
            ctx.advance_and_compare_stake(&stake, &stake_account).await;
            ctx.deactivate_stake(&stake, &mut stake_account, &staker)
                .await;
            ctx.advance_until_fully_deactivated(&stake, &stake_account, 20)
                .await;
        }
        "DuringCooldown" => {
            ctx.advance_until_fully_activated(&stake, &stake_account, 20)
                .await;
            ctx.deactivate_stake(&stake, &mut stake_account, &staker)
                .await;
            ctx.advance_and_compare_stake(&stake, &stake_account).await;
            ctx.advance_and_compare_stake(&stake, &stake_account).await;
        }
        _ => panic!("Invalid timing"),
    }

    // Redelegate to different validator
    ctx.delegate_stake_to(
        &stake,
        &mut stake_account,
        &staker,
        &vote_account_b,
        &vote_account_b_data,
    )
    .await;

    // Verify immediate state after redelegation
    ctx.compare_account_state(&stake, &stake_account).await;
    let banks_effective = ctx.get_banks_effective_stake(&stake).await;
    let mollusk_effective = ctx.get_mollusk_effective_stake(&stake_account);
    assert_eq!(
        banks_effective, mollusk_effective,
        "Effective stake mismatch immediately after redelegation"
    );

    // Complete activation with new validator
    ctx.advance_until_fully_activated(&stake, &stake_account, 20)
        .await;
}

// Test old warmup rate behavior (pre-SIMD-0093) with full lifecycle
#[tokio::test]
async fn test_old_warmup_rate_full_lifecycle() {
    let mut ctx = DualContext::new().await;

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    // Activate with old warmup rate
    for _ in 0..10 {
        ctx.advance_and_compare_stake_with_rate(&stake, &stake_account, None)
            .await;
    }

    // Deactivate with old warmup rate
    ctx.deactivate_stake(&stake, &mut stake_account, &staker)
        .await;

    for _ in 0..10 {
        ctx.advance_and_compare_stake_with_rate(&stake, &stake_account, None)
            .await;
    }
}

#[tokio::test]
async fn test_warmup_rate_transition() {
    let mut ctx = DualContext::new().await;

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(MINIMUM_DELEGATION, &Lockup::default())
        .await;

    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    for _ in 0..5 {
        ctx.advance_and_compare_stake_with_rate(&stake, &stake_account, None)
            .await;
    }

    let transition_epoch = ctx.mollusk.sysvars.clock.epoch;

    for _ in 0..10 {
        ctx.advance_and_compare_stake_with_rate(&stake, &stake_account, Some(transition_epoch))
            .await;
    }
}

// Test massive stake (50% of background) through full lifecycle
// This verifies that warmup/cooldown rates work correctly when new stake significantly
// impacts the existing effective stake, and ensures background stake is preserved
#[tokio::test]
async fn test_massive_stake_full_lifecycle() {
    let mut ctx = DualContext::new().await;

    // Get the background stake - use epoch 0's effective stake as the baseline
    let banks_stake_history = ctx.get_banks_stake_history().await;
    let background_effective = banks_stake_history
        .get(0)
        .map(|entry| entry.effective)
        .expect("Epoch 0 must have background stake");

    // Stake 50% of background effective stake - this is MASSIVE and will significantly
    // affect warmup rates (warmup is limited by new_stake relative to existing effective)
    let staked_amount = background_effective / 2;

    if staked_amount < MINIMUM_DELEGATION {
        panic!(
            "Bad test: background stake {} too small (need at least {})",
            background_effective,
            MINIMUM_DELEGATION * 2
        );
    }

    let (stake, mut stake_account, staker) = ctx
        .create_and_fund_stake(staked_amount, &Lockup::default())
        .await;

    // Activate
    ctx.delegate_stake(&stake, &mut stake_account, &staker)
        .await;

    let epochs_to_activate = ctx
        .advance_until_fully_activated(&stake, &stake_account, 50)
        .await;

    // Verify background stake preserved during activation
    ctx.verify_background_stake_across_epochs(epochs_to_activate, background_effective)
        .await;

    // Deactivate
    ctx.deactivate_stake(&stake, &mut stake_account, &staker)
        .await;

    let epochs_to_deactivate = ctx
        .advance_until_fully_deactivated(&stake, &stake_account, 50)
        .await;

    // Verify background stake preserved during deactivation
    ctx.verify_background_stake_across_epochs(epochs_to_deactivate, background_effective)
        .await;
}
