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
        state::{Authorized, Lockup, StakeStateV2},
    },
    solana_stake_program::id,
    solana_sysvar_id::SysvarId,
    solana_vote_interface::state::{VoteStateV4, VoteStateVersions},
    std::collections::HashMap,
};

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
        vote_account: &Pubkey,
        staked_amount: u64,
    ) -> (AccountSharedData, Pubkey, Pubkey) {
        let staker = Pubkey::new_unique();
        let withdrawer = Pubkey::new_unique();

        let account = self.create_stake_account_fully_specified(
            mollusk,
            vote_account,
            staked_amount,
            &staker,
            &withdrawer,
            &Lockup::default(),
        );

        (account, staker, withdrawer)
    }

    /// Create a stake account with full specification of authorities and lockup
    pub fn create_stake_account_fully_specified(
        self,
        mollusk: &mut Mollusk,
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
            let stake_pubkey = Pubkey::new_unique();
            let instruction = ixn::delegate_stake(&stake_pubkey, staker, vote_account);

            let accounts = vec![
                (stake_pubkey, stake_account.clone()),
                (*vote_account, create_vote_account()),
            ];

            // Use add_sysvars to provide clock, stake history, and config accounts
            let accounts_with_sysvars = add_sysvars(mollusk, &instruction, accounts);
            let result = mollusk.process_instruction(&instruction, &accounts_with_sysvars);
            stake_account = result.resulting_accounts[0].1.clone().into();
        }

        // For Activating lifecycle: NO epoch advance (stays transient at current epoch)
        // History management is handled by test-specific "true up" logic

        // Advance epoch to activate if needed (Active and beyond)
        if self >= StakeLifecycle::Active {
            // Get the activation_epoch that was set during delegation
            let stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();
            let activation_epoch = if let StakeStateV2::Stake(_, stake, _) = &stake_state {
                stake.delegation.activation_epoch
            } else {
                mollusk.sysvars.clock.epoch
            };

            // Advance 1 epoch to fully activate the stake (matches program_test behavior)
            let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;
            let current_slot = mollusk.sysvars.clock.slot;

            mollusk.warp_to_slot(current_slot + slots_per_epoch);

            // Add history for the activation epoch showing the stake as activating
            // Use a large baseline to ensure instant activation (matches program_test behavior)
            let mut stake_history = mollusk.sysvars.stake_history.clone();
            let existing = stake_history.get(activation_epoch).cloned();
            let existing_effective = existing.as_ref().map(|e| e.effective).unwrap_or(0);
            let existing_activating = existing.as_ref().map(|e| e.activating).unwrap_or(0);

            // Consolidate any prior activating
            let consolidated = existing_effective + existing_activating;

            // Add baseline for instant activation (13x the stake amount, or use existing if present)
            let baseline = if consolidated == 0 {
                staked_amount.saturating_mul(13)
            } else {
                consolidated
            };

            stake_history.add(
                activation_epoch,
                StakeHistoryEntry {
                    effective: baseline,
                    activating: staked_amount,
                    deactivating: 0,
                },
            );

            mollusk.sysvars.stake_history = stake_history;
        }

        // Deactivate if needed
        if self >= StakeLifecycle::Deactivating {
            let stake_pubkey = Pubkey::new_unique();
            let instruction = ixn::deactivate_stake(&stake_pubkey, staker);

            let accounts = vec![(stake_pubkey, stake_account.clone())];

            // Use add_sysvars to provide clock account
            let accounts_with_sysvars = add_sysvars(mollusk, &instruction, accounts);
            let result = mollusk.process_instruction(&instruction, &accounts_with_sysvars);
            stake_account = result.resulting_accounts[0].1.clone().into();
        }

        // For Deactivating lifecycle: NO epoch advance (stays transient at current epoch)
        // History management is handled by test-specific "true up" logic

        // Advance epoch to fully deactivate if needed
        // Advance epoch to fully deactivate if needed (Deactive lifecycle)
        // Matches program_test.rs line 978-983: advance_epoch once to fully deactivate
        if self == StakeLifecycle::Deactive {
            let deactivation_epoch = mollusk.sysvars.clock.epoch;

            // Advance 1 epoch to fully deactivate the stake (matches program_test behavior)
            let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;
            let current_slot = mollusk.sysvars.clock.slot;

            mollusk.warp_to_slot(current_slot + slots_per_epoch);

            // Add history for the deactivation epoch showing the stake as deactivating
            // Use a large baseline to ensure instant deactivation (matches program_test behavior)
            let mut stake_history = mollusk.sysvars.stake_history.clone();
            let existing = stake_history.get(deactivation_epoch).cloned();
            let existing_effective = existing.as_ref().map(|e| e.effective).unwrap_or(0);
            let existing_deactivating = existing.as_ref().map(|e| e.deactivating).unwrap_or(0);

            // Add baseline for instant deactivation (13x the stake amount, or use existing if present)
            let baseline = if existing_effective == 0 && existing_deactivating == 0 {
                staked_amount.saturating_mul(13)
            } else {
                existing_effective
            };

            stake_history.add(
                deactivation_epoch,
                StakeHistoryEntry {
                    effective: baseline,
                    activating: 0,
                    deactivating: existing_deactivating + staked_amount,
                },
            );

            mollusk.sysvars.stake_history = stake_history;
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

/// Advance to the next epoch and update stake history to show stake activation
///
/// This properly sets up stake history entries so that stakes can be seen as active
/// by the program when it checks via stake.stake() which uses the stake history sysvar.
///
/// IMPORTANT: Stake history should only contain entries for PAST epochs (not current).
/// The StakeHistorySysvar::get_entry assumes newest_historical_epoch = current_epoch - 1.
/// Add history showing a stake activated in the past WITHOUT advancing time
/// This allows individual Active stakes to have their own timeline without polluting shared history
pub fn add_activation_history(
    mollusk: &mut Mollusk,
    stake_amount: u64,
    activation_epoch_in_past: u64,
) {
    let mut stake_history = mollusk.sysvars.stake_history.clone();

    // Simply record that this stake amount existed as effective at that past epoch
    // Don't use any baseline - the stake appears active because it's been 25+ epochs since activation
    let existing = stake_history.get(activation_epoch_in_past).cloned();
    let existing_effective = existing.as_ref().map(|e| e.effective).unwrap_or(0);

    stake_history.add(
        activation_epoch_in_past,
        StakeHistoryEntry {
            effective: existing_effective + stake_amount,
            activating: 0, // Already fully activated (it's been 25 epochs)
            deactivating: 0,
        },
    );
    mollusk.sysvars.stake_history = stake_history;
}

/// Add history showing a stake deactivated in the past WITHOUT advancing time
pub fn add_deactivation_history(mollusk: &mut Mollusk, deactivation_epoch_in_past: u64) {
    let mut stake_history = mollusk.sysvars.stake_history.clone();

    // Record that deactivation completed at that past epoch
    // No effective stake remains (it's been 25+ epochs since deactivation)
    let existing = stake_history.get(deactivation_epoch_in_past).cloned();

    stake_history.add(
        deactivation_epoch_in_past,
        StakeHistoryEntry {
            effective: existing.as_ref().map(|e| e.effective).unwrap_or(0),
            activating: 0,
            deactivating: 0, // Already fully deactivated (it's been 25 epochs)
        },
    );
    mollusk.sysvars.stake_history = stake_history;
}

pub fn advance_epoch_and_activate_stake(
    mollusk: &mut Mollusk,
    stake_amount: u64,
    activation_epoch: u64,
) {
    // This function handles activation of a stake that was just delegated.
    // The stake was delegated at activation_epoch (should equal current_epoch).
    //
    // This matches program_test's advance_epoch() behavior: advance 1 epoch and add
    // minimal stake_history showing the stake as activating. The stake doesn't need
    // to be fully activated - just needs effective stake > 0 to trigger checks like
    // TooSoonToRedelegate.
    //
    // CRITICAL: Stake history should ONLY contain PAST epochs.

    let slots_per_epoch = mollusk.sysvars.epoch_schedule.slots_per_epoch;
    let current_slot = mollusk.sysvars.clock.slot;

    // Advance 1 epoch (matching program_test's advance_epoch)
    mollusk.warp_to_slot(current_slot + slots_per_epoch);

    // Add history for the activation_epoch (now in the past) showing this stake activating.
    // The baseline effective stake enables warmup calculation to show some effective stake
    // after 1 epoch, which is sufficient for delegation checks.
    let mut stake_history = mollusk.sysvars.stake_history.clone();

    let existing = stake_history.get(activation_epoch).cloned();
    let existing_effective = existing.as_ref().map(|e| e.effective).unwrap_or(0);
    let existing_activating = existing.as_ref().map(|e| e.activating).unwrap_or(0);

    // Consolidate any prior activating into effective
    let consolidated_effective = existing_effective + existing_activating;

    // Add baseline effective stake for warmup calculation (13x is moderate, works for all tests)
    // This allows the stake to show as partially active after 1 epoch, matching program_test
    let effective_with_baseline = if consolidated_effective == 0 {
        stake_amount.saturating_mul(13)
    } else {
        consolidated_effective
    };

    stake_history.add(
        activation_epoch,
        StakeHistoryEntry {
            effective: effective_with_baseline,
            activating: stake_amount,
            deactivating: 0,
        },
    );

    mollusk.sysvars.stake_history = stake_history;
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

/// True up a transient stake account's epoch to the current epoch
/// and optionally add stake history for the previous epoch
pub fn true_up_transient_stake_epoch(
    mollusk: &mut Mollusk,
    stake_account: &mut AccountSharedData,
    lifecycle: StakeLifecycle,
    stake_amount: u64,
    add_history: bool,
) {
    if lifecycle != StakeLifecycle::Activating && lifecycle != StakeLifecycle::Deactivating {
        return;
    }

    let clock = mollusk.sysvars.clock.clone();
    let mut stake_state: StakeStateV2 = bincode::deserialize(stake_account.data()).unwrap();

    let old_epoch = if let StakeStateV2::Stake(_, ref stake, _) = &stake_state {
        match lifecycle {
            StakeLifecycle::Activating => stake.delegation.activation_epoch,
            StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch,
            _ => clock.epoch,
        }
    } else {
        clock.epoch
    };

    if let StakeStateV2::Stake(_, ref mut stake, _) = &mut stake_state {
        match lifecycle {
            StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
            StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
            _ => (),
        }
    }
    stake_account.set_data(bincode::serialize(&stake_state).unwrap());

    if add_history && old_epoch != clock.epoch && old_epoch < clock.epoch {
        let mut stake_history = mollusk.sysvars.stake_history.clone();
        let existing = stake_history.get(old_epoch).cloned();

        match lifecycle {
            StakeLifecycle::Activating => {
                stake_history.add(
                    old_epoch,
                    StakeHistoryEntry {
                        effective: existing
                            .as_ref()
                            .map(|e| e.effective + e.activating)
                            .unwrap_or(0),
                        activating: stake_amount,
                        deactivating: 0,
                    },
                );
            }
            StakeLifecycle::Deactivating => {
                stake_history.add(
                    old_epoch,
                    StakeHistoryEntry {
                        effective: existing.as_ref().map(|e| e.effective).unwrap_or(0),
                        activating: 0,
                        deactivating: existing.as_ref().map(|e| e.deactivating).unwrap_or(0)
                            + stake_amount,
                    },
                );
            }
            _ => (),
        }
        mollusk.sysvars.stake_history = stake_history;
    }
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
