#![allow(clippy::arithmetic_side_effects)]
#![allow(dead_code)]

use {
    mollusk_svm::Mollusk,
    solana_account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
    solana_clock::Epoch,
    solana_instruction::Instruction,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{
        instruction as ixn,
        stake_history::{StakeHistory, StakeHistoryEntry},
        state::{Authorized, Delegation, Lockup, StakeStateV2},
    },
    solana_stake_program::id,
    solana_sysvar_id::SysvarId,
    solana_vote_interface::state::{VoteStateV4, VoteStateVersions},
    std::collections::HashMap,
};

// Inline the stake tracker instead of a separate module
// This replicates solana-runtime's Banks behavior where stake history is automatically
// updated at epoch boundaries by aggregating all stake delegations.

/// Tracks stake delegations for automatic stake history management
#[derive(Default, Clone)]
pub struct StakeTracker {
    /// Map of stake account pubkey to its delegation info
    pub(crate) delegations: HashMap<Pubkey, TrackedDelegation>,
}

#[derive(Clone)]
pub(crate) struct TrackedDelegation {
    stake: u64,
    activation_epoch: Epoch,
    deactivation_epoch: Epoch,
    voter_pubkey: Pubkey,
}

impl StakeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a tracker with background cluster stake (like Banks has)
    /// This provides the baseline effective stake that enables instant activation/deactivation
    pub fn with_background_stake(background_stake: u64) -> Self {
        let mut tracker = Self::new();

        // Add a synthetic background stake that's been active forever (bootstrap stake)
        // This mimics Banks' cluster-wide effective stake
        tracker.delegations.insert(
            Pubkey::new_unique(), // Synthetic background stake pubkey
            TrackedDelegation {
                stake: background_stake,
                activation_epoch: u64::MAX, // Bootstrap = instantly effective
                deactivation_epoch: u64::MAX,
                voter_pubkey: Pubkey::new_unique(),
            },
        );

        tracker
    }

    /// Track a new stake delegation (called after delegate instruction)
    pub fn track_delegation(
        &mut self,
        stake_pubkey: &Pubkey,
        stake_amount: u64,
        activation_epoch: Epoch,
        voter_pubkey: &Pubkey,
    ) {
        self.delegations.insert(
            *stake_pubkey,
            TrackedDelegation {
                stake: stake_amount,
                activation_epoch,
                deactivation_epoch: u64::MAX,
                voter_pubkey: *voter_pubkey,
            },
        );
    }

    /// Mark a stake as deactivating (called after deactivate instruction)
    pub fn track_deactivation(&mut self, stake_pubkey: &Pubkey, deactivation_epoch: Epoch) {
        if let Some(delegation) = self.delegations.get_mut(stake_pubkey) {
            delegation.deactivation_epoch = deactivation_epoch;
        }
    }

    /// Calculate aggregate stake history for an epoch (replicates Stakes::activate_epoch)
    fn calculate_epoch_entry(
        &self,
        epoch: Epoch,
        stake_history: &StakeHistory,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> StakeHistoryEntry {
        self.delegations
            .values()
            .map(|tracked| {
                let delegation = Delegation {
                    voter_pubkey: tracked.voter_pubkey,
                    stake: tracked.stake,
                    activation_epoch: tracked.activation_epoch,
                    deactivation_epoch: tracked.deactivation_epoch,
                    ..Delegation::default()
                };

                delegation.stake_activating_and_deactivating(
                    epoch,
                    stake_history,
                    new_rate_activation_epoch,
                )
            })
            .fold(StakeHistoryEntry::default(), |acc, status| {
                StakeHistoryEntry {
                    effective: acc.effective + status.effective,
                    activating: acc.activating + status.activating,
                    deactivating: acc.deactivating + status.deactivating,
                }
            })
    }
}

/// Extension trait that adds stake-aware warping to Mollusk
pub trait MolluskStakeExt {
    /// Warp to a slot and automatically update stake history at epoch boundaries
    ///
    /// This replicates Banks' behavior from solana-runtime:
    /// - Bank::warp_from_parent() advances slot
    /// - Stakes::activate_epoch() aggregates delegations
    /// - Bank::update_stake_history() writes sysvar
    fn warp_to_slot_with_stake_tracking(
        &mut self,
        tracker: &StakeTracker,
        target_slot: u64,
        new_rate_activation_epoch: Option<Epoch>,
    );
}

impl MolluskStakeExt for Mollusk {
    fn warp_to_slot_with_stake_tracking(
        &mut self,
        tracker: &StakeTracker,
        target_slot: u64,
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        let current_epoch = self.sysvars.clock.epoch;
        let current_slot = self.sysvars.clock.slot;

        if target_slot <= current_slot {
            panic!(
                "Cannot warp backwards: current_slot={}, target_slot={}",
                current_slot, target_slot
            );
        }

        // Advance the clock (Mollusk's warp_to_slot only updates Clock sysvar)
        self.warp_to_slot(target_slot);

        let new_epoch = self.sysvars.clock.epoch;

        // If we crossed epoch boundaries, update stake history for EACH epoch
        // StakeHistorySysvar requires contiguous history with no gaps
        // This replicates Bank::update_stake_history() + Stakes::activate_epoch()
        if new_epoch != current_epoch {
            for epoch in current_epoch..new_epoch {
                let entry = tracker.calculate_epoch_entry(
                    epoch,
                    &self.sysvars.stake_history,
                    new_rate_activation_epoch,
                );

                self.sysvars.stake_history.add(epoch, entry);
            }
        }
    }
}

// Hardcoded for convenience - matches interface.rs
pub const STAKE_RENT_EXEMPTION: u64 = 2_282_880;

#[test]
fn assert_stake_rent_exemption() {
    assert_eq!(
        Rent::default().minimum_balance(StakeStateV2::size_of()),
        STAKE_RENT_EXEMPTION
    );
}

/// Create a vote account with VoteStateV4
pub fn create_vote_account() -> AccountSharedData {
    let space = VoteStateV4::size_of();
    let lamports = Rent::default().minimum_balance(space);
    let vote_state = VoteStateVersions::new_v4(VoteStateV4::default());
    let data = bincode::serialize(&vote_state).unwrap();

    Account::create(lamports, data, solana_sdk_ids::vote::id(), false, u64::MAX).into()
}

/// Increment vote account credits
pub fn increment_vote_account_credits(
    vote_account: &mut AccountSharedData,
    epoch: Epoch,
    credits: u64,
) {
    let mut vote_state: VoteStateVersions = bincode::deserialize(vote_account.data()).unwrap();

    if let VoteStateVersions::V4(ref mut v4) = vote_state {
        v4.epoch_credits.push((epoch, credits, 0));
    }

    vote_account.set_data(bincode::serialize(&vote_state).unwrap());
}

/// Lifecycle states for stake accounts in tests
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
    /// Create a stake account at this lifecycle stage
    /// Returns (stake_account, staker_pubkey, withdrawer_pubkey)
    pub fn create_stake_account(
        self,
        mollusk: &mut Mollusk,
        tracker: &mut StakeTracker,
        stake_pubkey: &Pubkey,
        vote_account: &Pubkey,
        staked_amount: u64,
    ) -> (AccountSharedData, Pubkey, Pubkey) {
        let staker = Pubkey::new_unique();
        let withdrawer = Pubkey::new_unique();

        let account = self.create_stake_account_fully_specified(
            mollusk,
            tracker,
            stake_pubkey,
            vote_account,
            staked_amount,
            &staker,
            &withdrawer,
            &Lockup::default(),
        );

        (account, staker, withdrawer)
    }

    /// Helper to create tracker with appropriate background stake for tests
    /// Returns a tracker seeded with background cluster stake
    pub fn create_tracker_for_test(minimum_delegation: u64) -> StakeTracker {
        // Use a moderate background stake amount
        // This mimics Banks' cluster-wide effective stake from all validators
        // Calculation: needs to be >> test stakes to provide stable warmup base
        let background_stake = minimum_delegation.saturating_mul(100);
        StakeTracker::with_background_stake(background_stake)
    }

    /// Create a stake account with full specification of authorities and lockup
    #[allow(clippy::too_many_arguments)]
    pub fn create_stake_account_fully_specified(
        self,
        mollusk: &mut Mollusk,
        tracker: &mut StakeTracker,
        stake_pubkey: &Pubkey,
        vote_account: &Pubkey,
        staked_amount: u64,
        staker: &Pubkey,
        withdrawer: &Pubkey,
        lockup: &Lockup,
    ) -> AccountSharedData {
        let is_closed = self == StakeLifecycle::Closed;

        // Create base account
        let mut stake_account = if is_closed {
            let mut account = Account::create(STAKE_RENT_EXEMPTION, vec![], id(), false, u64::MAX);
            // Add staked_amount even for closed accounts (matches program-test behavior)
            if staked_amount > 0 {
                account.lamports += staked_amount;
            }
            account.into()
        } else {
            Account::create(
                STAKE_RENT_EXEMPTION + staked_amount,
                vec![0; StakeStateV2::size_of()],
                id(),
                false,
                u64::MAX,
            )
            .into()
        };

        if is_closed {
            return stake_account;
        }

        let authorized = Authorized {
            staker: *staker,
            withdrawer: *withdrawer,
        };

        // Initialize if needed
        if self >= StakeLifecycle::Initialized {
            let stake_state = StakeStateV2::Initialized(solana_stake_interface::state::Meta {
                rent_exempt_reserve: STAKE_RENT_EXEMPTION,
                authorized,
                lockup: *lockup,
            });
            bincode::serialize_into(stake_account.data_as_mut_slice(), &stake_state).unwrap();
        }

        // Delegate if needed
        if self >= StakeLifecycle::Activating {
            let instruction = ixn::delegate_stake(stake_pubkey, staker, vote_account);

            let accounts = vec![
                (*stake_pubkey, stake_account.clone()),
                (*vote_account, create_vote_account()),
            ];

            // Use add_sysvars to provide clock, stake history, and config accounts
            let accounts_with_sysvars = add_sysvars(mollusk, &instruction, accounts);
            let result = mollusk.process_instruction(&instruction, &accounts_with_sysvars);
            stake_account = result.resulting_accounts[0].1.clone().into();

            // Track delegation in the tracker
            let activation_epoch = mollusk.sysvars.clock.epoch;
            tracker.track_delegation(stake_pubkey, staked_amount, activation_epoch, vote_account);
        }

        // For Activating lifecycle: NO epoch advance (stays transient at current epoch)

        // Advance epoch to activate if needed (Active and beyond)
        if self >= StakeLifecycle::Active {
            // With background stake in tracker, just warp 1 epoch
            // The background stake provides baseline for instant partial activation
            let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;
            let current_slot = mollusk.sysvars.clock.slot;
            let target_slot = current_slot + slots_per_epoch;

            mollusk.warp_to_slot_with_stake_tracking(tracker, target_slot, Some(0));
        }

        // Deactivate if needed
        if self >= StakeLifecycle::Deactivating {
            let instruction = ixn::deactivate_stake(stake_pubkey, staker);

            let accounts = vec![(*stake_pubkey, stake_account.clone())];

            // Use add_sysvars to provide clock account
            let accounts_with_sysvars = add_sysvars(mollusk, &instruction, accounts);
            let result = mollusk.process_instruction(&instruction, &accounts_with_sysvars);
            stake_account = result.resulting_accounts[0].1.clone().into();

            // Track deactivation in the tracker
            let deactivation_epoch = mollusk.sysvars.clock.epoch;
            tracker.track_deactivation(stake_pubkey, deactivation_epoch);
        }

        // For Deactivating lifecycle: NO epoch advance (stays transient at current epoch)

        // Advance epoch to fully deactivate if needed (Deactive lifecycle)
        // Matches program_test.rs line 978-983: advance_epoch once to fully deactivate
        if self == StakeLifecycle::Deactive {
            // With background stake, advance 1 epoch for deactivation
            // Background provides the baseline for instant partial deactivation
            let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;
            let current_slot = mollusk.sysvars.clock.slot;
            let target_slot = current_slot + slots_per_epoch;

            mollusk.warp_to_slot_with_stake_tracking(tracker, target_slot, Some(0));
        }

        stake_account
    }

    /// Whether this lifecycle stage enforces minimum delegation for split
    pub fn split_minimum_enforced(&self) -> bool {
        matches!(
            self,
            Self::Activating | Self::Active | Self::Deactivating | Self::Deactive
        )
    }

    /// Whether this lifecycle stage enforces minimum delegation for withdraw
    pub fn withdraw_minimum_enforced(&self) -> bool {
        matches!(self, Self::Activating | Self::Active | Self::Deactivating)
    }
}

/// Get the effective stake for an account
pub fn get_effective_stake(mollusk: &Mollusk, stake_account: &AccountSharedData) -> u64 {
    let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();

    if let StakeStateV2::Stake(_, stake, _) = stake_state {
        stake
            .delegation
            .stake_activating_and_deactivating(
                mollusk.sysvars.clock.epoch,
                &mollusk.sysvars.stake_history,
                Some(0),
            )
            .effective
    } else {
        0
    }
}

/// Parse a stake account into (Meta, Option<Stake>, lamports)
pub fn parse_stake_account(
    stake_account: &AccountSharedData,
) -> (
    solana_stake_interface::state::Meta,
    Option<solana_stake_interface::state::Stake>,
    u64,
) {
    let lamports = stake_account.lamports();
    let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();

    match stake_state {
        StakeStateV2::Initialized(meta) => (meta, None, lamports),
        StakeStateV2::Stake(meta, stake, _) => (meta, Some(stake), lamports),
        _ => panic!("Expected initialized or staked account"),
    }
}

/// Resolve all accounts for an instruction, including sysvars and instruction accounts
/// This follows the pattern from interface.rs
///
/// This function re-serializes the stake history sysvar from mollusk.sysvars.stake_history
/// every time it's called, ensuring that any updates to the stake history are reflected in the accounts.
pub fn add_sysvars(
    mollusk: &Mollusk,
    instruction: &Instruction,
    accounts: Vec<(Pubkey, AccountSharedData)>,
) -> Vec<(Pubkey, Account)> {
    // Build a map of provided accounts
    let mut account_map: HashMap<Pubkey, Account> = accounts
        .into_iter()
        .map(|(pk, acc)| (pk, acc.into()))
        .collect();

    // Now resolve all accounts from the instruction
    let mut result = Vec::new();
    for account_meta in &instruction.accounts {
        let key = account_meta.pubkey;
        let account = if let Some(acc) = account_map.remove(&key) {
            // Use the provided account
            acc
        } else if Rent::check_id(&key) {
            mollusk.sysvars.keyed_account_for_rent_sysvar().1
        } else if solana_clock::Clock::check_id(&key) {
            mollusk.sysvars.keyed_account_for_clock_sysvar().1
        } else if solana_epoch_schedule::EpochSchedule::check_id(&key) {
            mollusk.sysvars.keyed_account_for_epoch_schedule_sysvar().1
        } else if solana_epoch_rewards::EpochRewards::check_id(&key) {
            mollusk.sysvars.keyed_account_for_epoch_rewards_sysvar().1
        } else if StakeHistory::check_id(&key) {
            // Re-serialize stake history from mollusk.sysvars.stake_history
            // to ensure updates are reflected
            mollusk.sysvars.keyed_account_for_stake_history_sysvar().1
        } else {
            // Default empty account
            // Note: stake_config is not provided, so get_minimum_delegation() returns 1
            Account::default()
        };

        result.push((key, account));
    }

    result
}

/// Initialize a stake account with the given authorities and lockup
pub fn initialize_stake_account(
    mollusk: &Mollusk,
    stake_pubkey: &Pubkey,
    lamports: u64,
    authorized: &Authorized,
    lockup: &Lockup,
) -> AccountSharedData {
    let stake_account = AccountSharedData::new_data_with_space(
        lamports,
        &StakeStateV2::Uninitialized,
        StakeStateV2::size_of(),
        &id(),
    )
    .unwrap();

    let instruction = ixn::initialize(stake_pubkey, authorized, lockup);
    let accounts = vec![(*stake_pubkey, stake_account)];
    let accounts_resolved = add_sysvars(mollusk, &instruction, accounts);
    let result = mollusk.process_instruction(&instruction, &accounts_resolved);

    result.resulting_accounts[0].1.clone().into()
}

/// Synchronize a transient stake's epoch to the current epoch
/// Updates both the account data and the tracker.
pub fn true_up_transient_stake_epoch(
    mollusk: &mut Mollusk,
    tracker: &mut StakeTracker,
    stake_pubkey: &Pubkey,
    stake_account: &mut AccountSharedData,
    lifecycle: StakeLifecycle,
) {
    if lifecycle != StakeLifecycle::Activating && lifecycle != StakeLifecycle::Deactivating {
        return;
    }

    let clock = mollusk.sysvars.clock.clone();
    let mut stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();

    if let StakeStateV2::Stake(_, ref mut stake, _) = &mut stake_state {
        match lifecycle {
            StakeLifecycle::Activating => {
                stake.delegation.activation_epoch = clock.epoch;

                // Update tracker as well
                if let Some(tracked) = tracker.delegations.get_mut(stake_pubkey) {
                    tracked.activation_epoch = clock.epoch;
                }
            }
            StakeLifecycle::Deactivating => {
                stake.delegation.deactivation_epoch = clock.epoch;

                // Update tracker as well
                tracker.track_deactivation(stake_pubkey, clock.epoch);
            }
            _ => (),
        }
    }
    stake_account.set_data(bincode::serialize(&stake_state).unwrap());
}

/// Test that removing any required signer causes the instruction to fail with MissingRequiredSignature,
/// then verify the instruction succeeds with all signers present.
///
/// NOTE: In mollusk, "signers" are controlled by the is_signer flag in AccountMeta. Unlike in
/// solana_program_test, we don't use Keypair objects - the runtime checks signatures based
/// on the instruction metadata.
pub fn process_instruction_after_testing_missing_signers(
    mollusk: &Mollusk,
    instruction: &Instruction,
    accounts: &[(Pubkey, AccountSharedData)],
    checks: &[mollusk_svm::result::Check],
) -> mollusk_svm::result::InstructionResult {
    use {mollusk_svm::result::Check, solana_program_error::ProgramError};

    for i in 0..instruction.accounts.len() {
        if instruction.accounts[i].is_signer {
            let mut modified_instruction = instruction.clone();
            modified_instruction.accounts[i].is_signer = false;

            let accounts_with_sysvars =
                add_sysvars(mollusk, &modified_instruction, accounts.to_vec());

            mollusk.process_and_validate_instruction(
                &modified_instruction,
                &accounts_with_sysvars,
                &[Check::err(ProgramError::MissingRequiredSignature)],
            );
        }
    }

    let accounts_with_sysvars = add_sysvars(mollusk, instruction, accounts.to_vec());
    mollusk.process_and_validate_instruction(instruction, &accounts_with_sysvars, checks)
}

/// Consolidated test context that bundles all common test setup
/// This eliminates 8-10 lines of boilerplate from every test
pub struct StakeTestContext {
    pub mollusk: Mollusk,
    pub tracker: StakeTracker,
    pub minimum_delegation: u64,
    pub rent_exempt_reserve: u64,
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
    pub vote_account: Pubkey,
    pub vote_account_data: AccountSharedData,
}

impl StakeTestContext {
    /// Create a new test context with all standard setup
    pub fn new() -> Self {
        let mollusk = Mollusk::new(&id(), "solana_stake_program");
        let minimum_delegation = solana_stake_program::get_minimum_delegation();
        let tracker = StakeLifecycle::create_tracker_for_test(minimum_delegation);

        Self {
            mollusk,
            tracker,
            minimum_delegation,
            rent_exempt_reserve: STAKE_RENT_EXEMPTION,
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
            vote_account: Pubkey::new_unique(),
            vote_account_data: create_vote_account(),
        }
    }

    /// Create a stake account at the specified lifecycle stage with standard authorities
    pub fn create_stake_account(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
    ) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = Pubkey::new_unique();
        let account = lifecycle.create_stake_account_fully_specified(
            &mut self.mollusk,
            &mut self.tracker,
            &stake_pubkey,
            &self.vote_account,
            staked_amount,
            &self.staker,
            &self.withdrawer,
            &Lockup::default(),
        );
        (stake_pubkey, account)
    }

    /// Create a stake account with custom lockup
    pub fn create_stake_account_with_lockup(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
        lockup: &Lockup,
    ) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = Pubkey::new_unique();
        let account = lifecycle.create_stake_account_fully_specified(
            &mut self.mollusk,
            &mut self.tracker,
            &stake_pubkey,
            &self.vote_account,
            staked_amount,
            &self.staker,
            &self.withdrawer,
            lockup,
        );
        (stake_pubkey, account)
    }

    /// Create a stake account with custom authorities
    pub fn create_stake_account_with_authorities(
        &mut self,
        lifecycle: StakeLifecycle,
        staked_amount: u64,
        staker: &Pubkey,
        withdrawer: &Pubkey,
    ) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = Pubkey::new_unique();
        let account = lifecycle.create_stake_account_fully_specified(
            &mut self.mollusk,
            &mut self.tracker,
            &stake_pubkey,
            &self.vote_account,
            staked_amount,
            staker,
            withdrawer,
            &Lockup::default(),
        );
        (stake_pubkey, account)
    }

    /// Create a lockup that expires in the future
    pub fn create_future_lockup(&self, epochs_ahead: u64) -> Lockup {
        Lockup {
            unix_timestamp: 0,
            epoch: self.mollusk.sysvars.clock.epoch + epochs_ahead,
            custodian: Pubkey::new_unique(),
        }
    }

    /// Create a lockup that's currently in force (far future)
    pub fn create_in_force_lockup(&self) -> Lockup {
        self.create_future_lockup(1_000_000)
    }

    /// Create a second vote account (for testing different vote accounts)
    pub fn create_second_vote_account(&self) -> (Pubkey, AccountSharedData) {
        (Pubkey::new_unique(), create_vote_account())
    }
}

impl Default for StakeTestContext {
    fn default() -> Self {
        Self::new()
    }
}
