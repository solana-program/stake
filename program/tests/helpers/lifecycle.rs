use {
    super::{
        stake_tracker::{MolluskStakeExt, StakeTracker},
        utils::{add_sysvars, create_vote_account, STAKE_RENT_EXEMPTION},
    },
    mollusk_svm::Mollusk,
    solana_account::{Account, AccountSharedData, WritableAccount},
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        instruction as ixn,
        state::{Authorized, Lockup, StakeStateV2},
    },
    solana_stake_program::id,
};

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
