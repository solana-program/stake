use {
    mollusk_svm::Mollusk,
    solana_clock::Epoch,
    solana_pubkey::Pubkey,
    solana_stake_interface::{
        stake_history::{StakeHistory, StakeHistoryEntry},
        state::Delegation,
    },
    std::collections::HashMap,
};

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
    pub(crate) stake: u64,
    pub(crate) activation_epoch: Epoch,
    pub(crate) deactivation_epoch: Epoch,
    pub(crate) voter_pubkey: Pubkey,
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
