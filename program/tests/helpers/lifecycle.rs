use {
    super::utils::STAKE_RENT_EXEMPTION, solana_account::AccountSharedData,
    solana_stake_interface::state::StakeStateV2, solana_stake_program::id,
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
    /// Create an uninitialized stake account
    pub fn create_uninitialized_account(self) -> AccountSharedData {
        AccountSharedData::new_data_with_space(
            STAKE_RENT_EXEMPTION,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .unwrap()
    }
}
