use {
    crate::instruction_builder::initialize::InitializeBuilder,
    crate::instruction_builder::split::SplitBuilder,
    crate::state::{Authorized, Lockup},
    solana_pubkey::Pubkey,
};

mod initialize;
mod split;

const RENT_ID: Pubkey = Pubkey::from_str_const("SysvarRent111111111111111111111111111111111");

/// The entry point for building Stake Program instructions
pub struct StakeInstructionBuilder;

impl StakeInstructionBuilder {
    pub fn initialize<'a>(
        stake_pubkey: &'a Pubkey,
        authorized: &'a Authorized,
        lockup: &'a Lockup,
    ) -> InitializeBuilder<'a> {
        InitializeBuilder::new(stake_pubkey, authorized, lockup)
    }

    pub fn split<'a>(
        stake_pubkey: &'a Pubkey,
        stake_authority_pubkey: &'a Pubkey,
        split_stake_pubkey: &'a Pubkey,
        lamports: u64,
    ) -> SplitBuilder<'a> {
        SplitBuilder::new(
            stake_pubkey,
            stake_authority_pubkey,
            split_stake_pubkey,
            lamports,
        )
    }

    // Would continue this pattern for all instructions
    // ...
    // ...
    // ...
    // ...
    // ...
    // ...
}
