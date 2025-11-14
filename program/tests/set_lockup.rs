#![allow(clippy::arithmetic_side_effects)]

mod helpers;

use {
    helpers::{
        context::StakeTestContext, instruction_builders::SetLockupCheckedConfig,
        lifecycle::StakeLifecycle,
    },
    mollusk_svm::result::Check,
    solana_pubkey::Pubkey,
    solana_stake_interface::instruction as ixn,
};

#[test]
fn test_set_lockup_checked() {
    let mut ctx = StakeTestContext::new();

    let custodian = Pubkey::new_unique();

    let (stake, initialized_stake_account) = ctx.stake_account(StakeLifecycle::Initialized).build();

    ctx.process_with(SetLockupCheckedConfig {
        stake: (&stake, &initialized_stake_account),
        lockup_args: &ixn::LockupArgs {
            unix_timestamp: None,
            epoch: Some(1),
            custodian: Some(custodian),
        },
        custodian: &ctx.withdrawer,
    })
    .checks(&[Check::success()])
    .test_missing_signers(true)
    .execute();
}
