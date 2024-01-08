//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::state` or `solana_program::stake::state` instead"
)]
pub use solana_sdk::stake::state::*;
use {
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        account_utils::StateMut,
        clock::{Clock, Epoch},
        feature_set::{self, FeatureSet},
        instruction::{checked_add, InstructionError},
        pubkey::Pubkey,
        rent::Rent,
        stake::{
            instruction::{LockupArgs, StakeError},
            program::id,
            stake_flags::StakeFlags,
            tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
        },
        stake_history::{StakeHistory, StakeHistoryEntry},
        transaction_context::{
            BorrowedAccount, IndexOfAccount, InstructionContext, TransactionContext,
        },
    },
    solana_vote_program::vote_state::{self, VoteState, VoteStateVersions},
    std::{cmp::Ordering, collections::HashSet, convert::TryFrom},
};

#[derive(Debug)]
pub enum SkippedReason {
    DisabledInflation,
    JustActivated,
    TooEarlyUnfairSplit,
    ZeroPoints,
    ZeroPointValue,
    ZeroReward,
    ZeroCreditsAndReturnZero,
    ZeroCreditsAndReturnCurrent,
    ZeroCreditsAndReturnRewinded,
}

impl From<SkippedReason> for InflationPointCalculationEvent {
    fn from(reason: SkippedReason) -> Self {
        InflationPointCalculationEvent::Skipped(reason)
    }
}

#[derive(Debug)]
pub enum InflationPointCalculationEvent {
    CalculatedPoints(u64, u128, u128, u128),
    SplitRewards(u64, u64, u64, PointValue),
    EffectiveStakeAtRewardedEpoch(u64),
    RentExemptReserve(u64),
    Delegation(Delegation, Pubkey),
    Commission(u8),
    CreditsObserved(u64, Option<u64>),
    Skipped(SkippedReason),
}

pub(crate) fn null_tracer() -> Option<impl Fn(&InflationPointCalculationEvent)> {
    None::<fn(&_)>
}

// utility function, used by Stakes, tests
pub fn from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<StakeStateV2> {
    account.state().ok()
}

pub fn stake_from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<Stake> {
    from(account).and_then(|state: StakeStateV2| state.stake())
}

pub fn delegation_from(account: &AccountSharedData) -> Option<Delegation> {
    from(account).and_then(|state: StakeStateV2| state.delegation())
}

pub fn authorized_from(account: &AccountSharedData) -> Option<Authorized> {
    from(account).and_then(|state: StakeStateV2| state.authorized())
}

pub fn lockup_from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<Lockup> {
    from(account).and_then(|state: StakeStateV2| state.lockup())
}

pub fn meta_from(account: &AccountSharedData) -> Option<Meta> {
    from(account).and_then(|state: StakeStateV2| state.meta())
}

pub(crate) fn new_warmup_cooldown_rate_epoch(invoke_context: &InvokeContext) -> Option<Epoch> {
    let epoch_schedule = invoke_context
        .get_sysvar_cache()
        .get_epoch_schedule()
        .unwrap();
    invoke_context
        .feature_set
        .new_warmup_cooldown_rate_epoch(epoch_schedule.as_ref())
}

fn get_stake_status(
    invoke_context: &InvokeContext,
    stake: &Stake,
    clock: &Clock,
) -> Result<StakeActivationStatus, InstructionError> {
    let stake_history = invoke_context.get_sysvar_cache().get_stake_history()?;
    Ok(stake.delegation.stake_activating_and_deactivating(
        clock.epoch,
        &stake_history,
        new_warmup_cooldown_rate_epoch(invoke_context),
    ))
}

fn redelegate_stake(
    invoke_context: &InvokeContext,
    stake: &mut Stake,
    stake_lamports: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    clock: &Clock,
    stake_history: &StakeHistory,
) -> Result<(), StakeError> {
    let new_rate_activation_epoch = new_warmup_cooldown_rate_epoch(invoke_context);
    // If stake is currently active:
    if stake.stake(clock.epoch, stake_history, new_rate_activation_epoch) != 0 {
        let stake_lamports_ok = if invoke_context
            .feature_set
            .is_active(&feature_set::stake_redelegate_instruction::id())
        {
            // When a stake account is redelegated, the delegated lamports from the source stake
            // account are transferred to a new stake account. Do not permit the deactivation of
            // the source stake account to be rescinded, by more generally requiring the delegation
            // be configured with the expected amount of stake lamports before rescinding.
            stake_lamports >= stake.delegation.stake
        } else {
            true
        };

        // If pubkey of new voter is the same as current,
        // and we are scheduled to start deactivating this epoch,
        // we rescind deactivation
        if stake.delegation.voter_pubkey == *voter_pubkey
            && clock.epoch == stake.delegation.deactivation_epoch
            && stake_lamports_ok
        {
            stake.delegation.deactivation_epoch = std::u64::MAX;
            return Ok(());
        } else {
            // can't redelegate to another pubkey if stake is active.
            return Err(StakeError::TooSoonToRedelegate);
        }
    }
    // Either the stake is freshly activated, is active but has been
    // deactivated this epoch, or has fully de-activated.
    // Redelegation implies either re-activation or un-deactivation

    stake.delegation.stake = stake_lamports;
    stake.delegation.activation_epoch = clock.epoch;
    stake.delegation.deactivation_epoch = std::u64::MAX;
    stake.delegation.voter_pubkey = *voter_pubkey;
    stake.credits_observed = vote_state.credits();
    Ok(())
}

pub(crate) fn new_stake(
    stake: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    activation_epoch: Epoch,
) -> Stake {
    Stake {
        delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
        credits_observed: vote_state.credits(),
    }
}

/// captures a rewards round as lamports to be awarded
///  and the total points over which those lamports
///  are to be distributed
//  basically read as rewards/points, but in integers instead of as an f64
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointValue {
    pub rewards: u64, // lamports to split
    pub points: u128, // over these points
}

fn redeem_stake_rewards(
    rewarded_epoch: Epoch,
    stake: &mut Stake,
    point_value: &PointValue,
    vote_state: &VoteState,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> Option<(u64, u64)> {
    if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
        inflation_point_calc_tracer(&InflationPointCalculationEvent::CreditsObserved(
            stake.credits_observed,
            None,
        ));
    }
    calculate_stake_rewards(
        rewarded_epoch,
        stake,
        point_value,
        vote_state,
        stake_history,
        inflation_point_calc_tracer.as_ref(),
        new_rate_activation_epoch,
    )
    .map(|calculated_stake_rewards| {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer {
            inflation_point_calc_tracer(&InflationPointCalculationEvent::CreditsObserved(
                stake.credits_observed,
                Some(calculated_stake_rewards.new_credits_observed),
            ));
        }
        stake.credits_observed = calculated_stake_rewards.new_credits_observed;
        stake.delegation.stake += calculated_stake_rewards.staker_rewards;
        (
            calculated_stake_rewards.staker_rewards,
            calculated_stake_rewards.voter_rewards,
        )
    })
}

fn calculate_stake_points(
    stake: &Stake,
    vote_state: &VoteState,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> u128 {
    calculate_stake_points_and_credits(
        stake,
        vote_state,
        stake_history,
        inflation_point_calc_tracer,
        new_rate_activation_epoch,
    )
    .points
}

#[derive(Debug, PartialEq, Eq)]
struct CalculatedStakePoints {
    points: u128,
    new_credits_observed: u64,
    force_credits_update_with_skipped_reward: bool,
}

/// for a given stake and vote_state, calculate how many
///   points were earned (credits * stake) and new value
///   for credits_observed were the points paid
fn calculate_stake_points_and_credits(
    stake: &Stake,
    new_vote_state: &VoteState,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> CalculatedStakePoints {
    let credits_in_stake = stake.credits_observed;
    let credits_in_vote = new_vote_state.credits();
    // if there is no newer credits since observed, return no point
    match credits_in_vote.cmp(&credits_in_stake) {
        Ordering::Less => {
            if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
                inflation_point_calc_tracer(&SkippedReason::ZeroCreditsAndReturnRewinded.into());
            }
            // Don't adjust stake.activation_epoch for simplicity:
            //  - generally fast-forwarding stake.activation_epoch forcibly (for
            //    artificial re-activation with re-warm-up) skews the stake
            //    history sysvar. And properly handling all the cases
            //    regarding deactivation epoch/warm-up/cool-down without
            //    introducing incentive skew is hard.
            //  - Conceptually, it should be acceptable for the staked SOLs at
            //    the recreated vote to receive rewards again immediately after
            //    rewind even if it looks like instant activation. That's
            //    because it must have passed the required warmed-up at least
            //    once in the past already
            //  - Also such a stake account remains to be a part of overall
            //    effective stake calculation even while the vote account is
            //    missing for (indefinite) time or remains to be pre-remove
            //    credits score. It should be treated equally to staking with
            //    delinquent validator with no differentiation.

            // hint with true to indicate some exceptional credits handling is needed
            return CalculatedStakePoints {
                points: 0,
                new_credits_observed: credits_in_vote,
                force_credits_update_with_skipped_reward: true,
            };
        }
        Ordering::Equal => {
            if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
                inflation_point_calc_tracer(&SkippedReason::ZeroCreditsAndReturnCurrent.into());
            }
            // don't hint caller and return current value if credits remain unchanged (= delinquent)
            return CalculatedStakePoints {
                points: 0,
                new_credits_observed: credits_in_stake,
                force_credits_update_with_skipped_reward: false,
            };
        }
        Ordering::Greater => {}
    }

    let mut points = 0;
    let mut new_credits_observed = credits_in_stake;

    for (epoch, final_epoch_credits, initial_epoch_credits) in
        new_vote_state.epoch_credits().iter().copied()
    {
        let stake_amount = u128::from(stake.delegation.stake(
            epoch,
            stake_history,
            new_rate_activation_epoch,
        ));

        // figure out how much this stake has seen that
        //   for which the vote account has a record
        let earned_credits = if credits_in_stake < initial_epoch_credits {
            // the staker observed the entire epoch
            final_epoch_credits - initial_epoch_credits
        } else if credits_in_stake < final_epoch_credits {
            // the staker registered sometime during the epoch, partial credit
            final_epoch_credits - new_credits_observed
        } else {
            // the staker has already observed or been redeemed this epoch
            //  or was activated after this epoch
            0
        };
        let earned_credits = u128::from(earned_credits);

        // don't want to assume anything about order of the iterator...
        new_credits_observed = new_credits_observed.max(final_epoch_credits);

        // finally calculate points for this epoch
        let earned_points = stake_amount * earned_credits;
        points += earned_points;

        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&InflationPointCalculationEvent::CalculatedPoints(
                epoch,
                stake_amount,
                earned_credits,
                earned_points,
            ));
        }
    }

    CalculatedStakePoints {
        points,
        new_credits_observed,
        force_credits_update_with_skipped_reward: false,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CalculatedStakeRewards {
    staker_rewards: u64,
    voter_rewards: u64,
    new_credits_observed: u64,
}

/// for a given stake and vote_state, calculate what distributions and what updates should be made
/// returns a tuple in the case of a payout of:
///   * staker_rewards to be distributed
///   * voter_rewards to be distributed
///   * new value for credits_observed in the stake
/// returns None if there's no payout or if any deserved payout is < 1 lamport
fn calculate_stake_rewards(
    rewarded_epoch: Epoch,
    stake: &Stake,
    point_value: &PointValue,
    vote_state: &VoteState,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> Option<CalculatedStakeRewards> {
    // ensure to run to trigger (optional) inflation_point_calc_tracer
    let CalculatedStakePoints {
        points,
        new_credits_observed,
        mut force_credits_update_with_skipped_reward,
    } = calculate_stake_points_and_credits(
        stake,
        vote_state,
        stake_history,
        inflation_point_calc_tracer.as_ref(),
        new_rate_activation_epoch,
    );

    // Drive credits_observed forward unconditionally when rewards are disabled
    // or when this is the stake's activation epoch
    if point_value.rewards == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::DisabledInflation.into());
        }
        force_credits_update_with_skipped_reward = true;
    } else if stake.delegation.activation_epoch == rewarded_epoch {
        // not assert!()-ed; but points should be zero
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::JustActivated.into());
        }
        force_credits_update_with_skipped_reward = true;
    }

    if force_credits_update_with_skipped_reward {
        return Some(CalculatedStakeRewards {
            staker_rewards: 0,
            voter_rewards: 0,
            new_credits_observed,
        });
    }

    if points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroPoints.into());
        }
        return None;
    }
    if point_value.points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroPointValue.into());
        }
        return None;
    }

    let rewards = points
        .checked_mul(u128::from(point_value.rewards))
        .unwrap()
        .checked_div(point_value.points)
        .unwrap();

    let rewards = u64::try_from(rewards).unwrap();

    // don't bother trying to split if fractional lamports got truncated
    if rewards == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroReward.into());
        }
        return None;
    }
    let (voter_rewards, staker_rewards, is_split) = vote_state.commission_split(rewards);
    if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
        inflation_point_calc_tracer(&InflationPointCalculationEvent::SplitRewards(
            rewards,
            voter_rewards,
            staker_rewards,
            (*point_value).clone(),
        ));
    }

    if (voter_rewards == 0 || staker_rewards == 0) && is_split {
        // don't collect if we lose a whole lamport somewhere
        //  is_split means there should be tokens on both sides,
        //  uncool to move credits_observed if one side didn't get paid
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::TooEarlyUnfairSplit.into());
        }
        return None;
    }

    Some(CalculatedStakeRewards {
        staker_rewards,
        voter_rewards,
        new_credits_observed,
    })
}

pub fn initialize(
    stake_account: &mut BorrowedAccount,
    authorized: &Authorized,
    lockup: &Lockup,
    rent: &Rent,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    if stake_account.get_data().len() != StakeStateV2::size_of() {
        return Err(InstructionError::InvalidAccountData);
    }

    if let StakeStateV2::Uninitialized = stake_account.get_state()? {
        let rent_exempt_reserve = rent.minimum_balance(stake_account.get_data().len());
        if stake_account.get_lamports() >= rent_exempt_reserve {
            stake_account.set_state(
                &StakeStateV2::Initialized(Meta {
                    rent_exempt_reserve,
                    authorized: *authorized,
                    lockup: *lockup,
                }),
                feature_set,
            )
        } else {
            Err(InstructionError::InsufficientFunds)
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

/// Authorize the given pubkey to manage stake (deactivate, withdraw). This may be called
/// multiple times, but will implicitly withdraw authorization from the previously authorized
/// staker. The default staker is the owner of the stake account's pubkey.
pub fn authorize(
    stake_account: &mut BorrowedAccount,
    signers: &HashSet<Pubkey>,
    new_authority: &Pubkey,
    stake_authorize: StakeAuthorize,
    clock: &Clock,
    custodian: Option<&Pubkey>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    match stake_account.get_state()? {
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.authorized.authorize(
                signers,
                new_authority,
                stake_authorize,
                Some((&meta.lockup, clock, custodian)),
            )?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags), feature_set)
        }
        StakeStateV2::Initialized(mut meta) => {
            meta.authorized.authorize(
                signers,
                new_authority,
                stake_authorize,
                Some((&meta.lockup, clock, custodian)),
            )?;
            stake_account.set_state(&StakeStateV2::Initialized(meta), feature_set)
        }
        _ => Err(InstructionError::InvalidAccountData),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn authorize_with_seed(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account: &mut BorrowedAccount,
    authority_base_index: IndexOfAccount,
    authority_seed: &str,
    authority_owner: &Pubkey,
    new_authority: &Pubkey,
    stake_authorize: StakeAuthorize,
    clock: &Clock,
    custodian: Option<&Pubkey>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let mut signers = HashSet::default();
    if instruction_context.is_instruction_account_signer(authority_base_index)? {
        let base_pubkey = transaction_context.get_key_of_account_at_index(
            instruction_context
                .get_index_of_instruction_account_in_transaction(authority_base_index)?,
        )?;
        signers.insert(Pubkey::create_with_seed(
            base_pubkey,
            authority_seed,
            authority_owner,
        )?);
    }
    authorize(
        stake_account,
        &signers,
        new_authority,
        stake_authorize,
        clock,
        custodian,
        feature_set,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn delegate(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    vote_account_index: IndexOfAccount,
    clock: &Clock,
    stake_history: &StakeHistory,
    signers: &HashSet<Pubkey>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, vote_account_index)?;
    if *vote_account.get_owner() != solana_vote_program::id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    let vote_pubkey = *vote_account.get_key();
    let vote_state = vote_account.get_state::<VoteStateVersions>();
    drop(vote_account);

    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    match stake_account.get_state()? {
        StakeStateV2::Initialized(meta) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(&stake_account, &meta, feature_set)?;
            let stake = new_stake(
                stake_amount,
                &vote_pubkey,
                &vote_state?.convert_to_current(),
                clock.epoch,
            );
            stake_account.set_state(
                &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
                feature_set,
            )
        }
        StakeStateV2::Stake(meta, mut stake, stake_flags) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(&stake_account, &meta, feature_set)?;
            redelegate_stake(
                invoke_context,
                &mut stake,
                stake_amount,
                &vote_pubkey,
                &vote_state?.convert_to_current(),
                clock,
                stake_history,
            )?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags), feature_set)
        }
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn deactivate_stake(
    invoke_context: &InvokeContext,
    stake: &mut Stake,
    stake_flags: &mut StakeFlags,
    epoch: Epoch,
) -> Result<(), InstructionError> {
    if invoke_context
        .feature_set
        .is_active(&feature_set::stake_redelegate_instruction::id())
    {
        if stake_flags.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED) {
            let stake_history = invoke_context.get_sysvar_cache().get_stake_history()?;
            // when MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED flag is set on stake_flags,
            // deactivation is only permitted when the stake delegation activating amount is zero.
            let status = stake.delegation.stake_activating_and_deactivating(
                epoch,
                &stake_history,
                new_warmup_cooldown_rate_epoch(invoke_context),
            );
            if status.activating != 0 {
                Err(InstructionError::from(
                    StakeError::RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted,
                ))
            } else {
                stake.deactivate(epoch)?;
                // After deactivation, need to clear `MustFullyActivateBeforeDeactivationIsPermitted` flag if any.
                // So that future activation and deactivation are not subject to that restriction.
                stake_flags
                    .remove(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
                Ok(())
            }
        } else {
            stake.deactivate(epoch)?;
            Ok(())
        }
    } else {
        stake.deactivate(epoch)?;
        Ok(())
    }
}

pub fn deactivate(
    invoke_context: &InvokeContext,
    stake_account: &mut BorrowedAccount,
    clock: &Clock,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    if let StakeStateV2::Stake(meta, mut stake, mut stake_flags) = stake_account.get_state()? {
        meta.authorized.check(signers, StakeAuthorize::Staker)?;
        deactivate_stake(invoke_context, &mut stake, &mut stake_flags, clock.epoch)?;
        stake_account.set_state(
            &StakeStateV2::Stake(meta, stake, stake_flags),
            &invoke_context.feature_set,
        )
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

pub fn set_lockup(
    stake_account: &mut BorrowedAccount,
    lockup: &LockupArgs,
    signers: &HashSet<Pubkey>,
    clock: &Clock,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    match stake_account.get_state()? {
        StakeStateV2::Initialized(mut meta) => {
            meta.set_lockup(lockup, signers, clock)?;
            stake_account.set_state(&StakeStateV2::Initialized(meta), feature_set)
        }
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.set_lockup(lockup, signers, clock)?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags), feature_set)
        }
        _ => Err(InstructionError::InvalidAccountData),
    }
}

pub fn split(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    lamports: u64,
    split_index: IndexOfAccount,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    let split =
        instruction_context.try_borrow_instruction_account(transaction_context, split_index)?;
    if *split.get_owner() != id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    if split.get_data().len() != StakeStateV2::size_of() {
        return Err(InstructionError::InvalidAccountData);
    }
    if !matches!(split.get_state()?, StakeStateV2::Uninitialized) {
        return Err(InstructionError::InvalidAccountData);
    }
    let split_lamport_balance = split.get_lamports();
    drop(split);
    let stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    if lamports > stake_account.get_lamports() {
        return Err(InstructionError::InsufficientFunds);
    }
    let stake_state = stake_account.get_state()?;
    drop(stake_account);

    match stake_state {
        StakeStateV2::Stake(meta, mut stake, stake_flags) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let minimum_delegation = crate::get_minimum_delegation(&invoke_context.feature_set);
            let is_active = if invoke_context
                .feature_set
                .is_active(&feature_set::require_rent_exempt_split_destination::id())
            {
                let clock = invoke_context.get_sysvar_cache().get_clock()?;
                let status = get_stake_status(invoke_context, &stake, &clock)?;
                status.effective > 0
            } else {
                false
            };
            let validated_split_info = validate_split_amount(
                invoke_context,
                transaction_context,
                instruction_context,
                stake_account_index,
                split_index,
                lamports,
                &meta,
                minimum_delegation,
                is_active,
            )?;

            // split the stake, subtract rent_exempt_balance unless
            // the destination account already has those lamports
            // in place.
            // this means that the new stake account will have a stake equivalent to
            // lamports minus rent_exempt_reserve if it starts out with a zero balance
            let (remaining_stake_delta, split_stake_amount) =
                if validated_split_info.source_remaining_balance == 0 {
                    // If split amount equals the full source stake (as implied by 0
                    // source_remaining_balance), the new split stake must equal the same
                    // amount, regardless of any current lamport balance in the split account.
                    // Since split accounts retain the state of their source account, this
                    // prevents any magic activation of stake by prefunding the split account.
                    //
                    // The new split stake also needs to ignore any positive delta between the
                    // original rent_exempt_reserve and the split_rent_exempt_reserve, in order
                    // to prevent magic activation of stake by splitting between accounts of
                    // different sizes.
                    let remaining_stake_delta = lamports.saturating_sub(meta.rent_exempt_reserve);
                    (remaining_stake_delta, remaining_stake_delta)
                } else {
                    // Otherwise, the new split stake should reflect the entire split
                    // requested, less any lamports needed to cover the split_rent_exempt_reserve.

                    if stake.delegation.stake.saturating_sub(lamports) < minimum_delegation {
                        return Err(StakeError::InsufficientDelegation.into());
                    }

                    (
                        lamports,
                        lamports.saturating_sub(
                            validated_split_info
                                .destination_rent_exempt_reserve
                                .saturating_sub(split_lamport_balance),
                        ),
                    )
                };

            if split_stake_amount < minimum_delegation {
                return Err(StakeError::InsufficientDelegation.into());
            }

            let split_stake = stake.split(remaining_stake_delta, split_stake_amount)?;
            let mut split_meta = meta;
            split_meta.rent_exempt_reserve = validated_split_info.destination_rent_exempt_reserve;

            let mut stake_account = instruction_context
                .try_borrow_instruction_account(transaction_context, stake_account_index)?;
            stake_account.set_state(
                &StakeStateV2::Stake(meta, stake, stake_flags),
                &invoke_context.feature_set,
            )?;
            drop(stake_account);
            let mut split = instruction_context
                .try_borrow_instruction_account(transaction_context, split_index)?;
            split.set_state(
                &StakeStateV2::Stake(split_meta, split_stake, stake_flags),
                &invoke_context.feature_set,
            )?;
        }
        StakeStateV2::Initialized(meta) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let validated_split_info = validate_split_amount(
                invoke_context,
                transaction_context,
                instruction_context,
                stake_account_index,
                split_index,
                lamports,
                &meta,
                0, // additional_required_lamports
                false,
            )?;
            let mut split_meta = meta;
            split_meta.rent_exempt_reserve = validated_split_info.destination_rent_exempt_reserve;
            let mut split = instruction_context
                .try_borrow_instruction_account(transaction_context, split_index)?;
            split.set_state(
                &StakeStateV2::Initialized(split_meta),
                &invoke_context.feature_set,
            )?;
        }
        StakeStateV2::Uninitialized => {
            let stake_pubkey = transaction_context.get_key_of_account_at_index(
                instruction_context
                    .get_index_of_instruction_account_in_transaction(stake_account_index)?,
            )?;
            if !signers.contains(stake_pubkey) {
                return Err(InstructionError::MissingRequiredSignature);
            }
        }
        _ => return Err(InstructionError::InvalidAccountData),
    }

    // Deinitialize state upon zero balance
    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    if lamports == stake_account.get_lamports() {
        stake_account.set_state(&StakeStateV2::Uninitialized, &invoke_context.feature_set)?;
    }
    drop(stake_account);

    let mut split =
        instruction_context.try_borrow_instruction_account(transaction_context, split_index)?;
    split.checked_add_lamports(lamports, &invoke_context.feature_set)?;
    drop(split);
    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    stake_account.checked_sub_lamports(lamports, &invoke_context.feature_set)?;
    Ok(())
}

pub fn merge(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    source_account_index: IndexOfAccount,
    clock: &Clock,
    stake_history: &StakeHistory,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    let mut source_account = instruction_context
        .try_borrow_instruction_account(transaction_context, source_account_index)?;
    // Ensure source isn't spoofed
    if *source_account.get_owner() != id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    // Close the stake_account-reference loophole
    if instruction_context.get_index_of_instruction_account_in_transaction(stake_account_index)?
        == instruction_context
            .get_index_of_instruction_account_in_transaction(source_account_index)?
    {
        return Err(InstructionError::InvalidArgument);
    }
    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;

    ic_msg!(invoke_context, "Checking if destination stake is mergeable");
    let stake_merge_kind = MergeKind::get_if_mergeable(
        invoke_context,
        &stake_account.get_state()?,
        stake_account.get_lamports(),
        clock,
        stake_history,
    )?;

    // Authorized staker is allowed to split/merge accounts
    stake_merge_kind
        .meta()
        .authorized
        .check(signers, StakeAuthorize::Staker)?;

    ic_msg!(invoke_context, "Checking if source stake is mergeable");
    let source_merge_kind = MergeKind::get_if_mergeable(
        invoke_context,
        &source_account.get_state()?,
        source_account.get_lamports(),
        clock,
        stake_history,
    )?;

    ic_msg!(invoke_context, "Merging stake accounts");
    if let Some(merged_state) = stake_merge_kind.merge(invoke_context, source_merge_kind, clock)? {
        stake_account.set_state(&merged_state, &invoke_context.feature_set)?;
    }

    // Source is about to be drained, deinitialize its state
    source_account.set_state(&StakeStateV2::Uninitialized, &invoke_context.feature_set)?;

    // Drain the source stake account
    let lamports = source_account.get_lamports();
    source_account.checked_sub_lamports(lamports, &invoke_context.feature_set)?;
    stake_account.checked_add_lamports(lamports, &invoke_context.feature_set)?;
    Ok(())
}

pub fn redelegate(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account: &mut BorrowedAccount,
    uninitialized_stake_account_index: IndexOfAccount,
    vote_account_index: IndexOfAccount,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    let clock = invoke_context.get_sysvar_cache().get_clock()?;

    // ensure `uninitialized_stake_account_index` is in the uninitialized state
    let mut uninitialized_stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, uninitialized_stake_account_index)?;
    if *uninitialized_stake_account.get_owner() != id() {
        ic_msg!(
            invoke_context,
            "expected uninitialized stake account owner to be {}, not {}",
            id(),
            *uninitialized_stake_account.get_owner()
        );
        return Err(InstructionError::IncorrectProgramId);
    }
    if uninitialized_stake_account.get_data().len() != StakeStateV2::size_of() {
        ic_msg!(
            invoke_context,
            "expected uninitialized stake account data len to be {}, not {}",
            StakeStateV2::size_of(),
            uninitialized_stake_account.get_data().len()
        );
        return Err(InstructionError::InvalidAccountData);
    }
    if !matches!(
        uninitialized_stake_account.get_state()?,
        StakeStateV2::Uninitialized
    ) {
        ic_msg!(
            invoke_context,
            "expected uninitialized stake account to be uninitialized",
        );
        return Err(InstructionError::AccountAlreadyInitialized);
    }

    // validate the provided vote account
    let vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, vote_account_index)?;
    if *vote_account.get_owner() != solana_vote_program::id() {
        ic_msg!(
            invoke_context,
            "expected vote account owner to be {}, not {}",
            solana_vote_program::id(),
            *vote_account.get_owner()
        );
        return Err(InstructionError::IncorrectProgramId);
    }
    let vote_pubkey = *vote_account.get_key();
    let vote_state = vote_account.get_state::<VoteStateVersions>()?;

    let (stake_meta, effective_stake) =
        if let StakeStateV2::Stake(meta, stake, _stake_flags) = stake_account.get_state()? {
            let status = get_stake_status(invoke_context, &stake, &clock)?;
            if status.effective == 0 || status.activating != 0 || status.deactivating != 0 {
                ic_msg!(invoke_context, "stake is not active");
                return Err(StakeError::RedelegateTransientOrInactiveStake.into());
            }

            // Deny redelegating to the same vote account. This is nonsensical and could be used to
            // grief the global stake warm-up/cool-down rate
            if stake.delegation.voter_pubkey == vote_pubkey {
                ic_msg!(
                    invoke_context,
                    "redelegating to the same vote account not permitted"
                );
                return Err(StakeError::RedelegateToSameVoteAccount.into());
            }

            (meta, status.effective)
        } else {
            ic_msg!(invoke_context, "invalid stake account data",);
            return Err(InstructionError::InvalidAccountData);
        };

    // deactivate `stake_account`
    //
    // Note: This function also ensures `signers` contains the `StakeAuthorize::Staker`
    deactivate(invoke_context, stake_account, &clock, signers)?;

    // transfer the effective stake to the uninitialized stake account
    stake_account.checked_sub_lamports(effective_stake, &invoke_context.feature_set)?;
    uninitialized_stake_account
        .checked_add_lamports(effective_stake, &invoke_context.feature_set)?;

    // initialize and schedule `uninitialized_stake_account` for activation
    let sysvar_cache = invoke_context.get_sysvar_cache();
    let rent = sysvar_cache.get_rent()?;
    let mut uninitialized_stake_meta = stake_meta;
    uninitialized_stake_meta.rent_exempt_reserve =
        rent.minimum_balance(uninitialized_stake_account.get_data().len());

    let ValidatedDelegatedInfo { stake_amount } = validate_delegated_amount(
        &uninitialized_stake_account,
        &uninitialized_stake_meta,
        &invoke_context.feature_set,
    )?;
    uninitialized_stake_account.set_state(
        &StakeStateV2::Stake(
            uninitialized_stake_meta,
            new_stake(
                stake_amount,
                &vote_pubkey,
                &vote_state.convert_to_current(),
                clock.epoch,
            ),
            StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED,
        ),
        &invoke_context.feature_set,
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn withdraw(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    lamports: u64,
    to_index: IndexOfAccount,
    clock: &Clock,
    stake_history: &StakeHistory,
    withdraw_authority_index: IndexOfAccount,
    custodian_index: Option<IndexOfAccount>,
    new_rate_activation_epoch: Option<Epoch>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let withdraw_authority_pubkey = transaction_context.get_key_of_account_at_index(
        instruction_context
            .get_index_of_instruction_account_in_transaction(withdraw_authority_index)?,
    )?;
    if !instruction_context.is_instruction_account_signer(withdraw_authority_index)? {
        return Err(InstructionError::MissingRequiredSignature);
    }
    let mut signers = HashSet::new();
    signers.insert(*withdraw_authority_pubkey);

    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    let (lockup, reserve, is_staked) = match stake_account.get_state()? {
        StakeStateV2::Stake(meta, stake, _stake_flag) => {
            meta.authorized
                .check(&signers, StakeAuthorize::Withdrawer)?;
            // if we have a deactivation epoch and we're in cooldown
            let staked = if clock.epoch >= stake.delegation.deactivation_epoch {
                stake
                    .delegation
                    .stake(clock.epoch, stake_history, new_rate_activation_epoch)
            } else {
                // Assume full stake if the stake account hasn't been
                //  de-activated, because in the future the exposed stake
                //  might be higher than stake.stake() due to warmup
                stake.delegation.stake
            };

            let staked_and_reserve = checked_add(staked, meta.rent_exempt_reserve)?;
            (meta.lockup, staked_and_reserve, staked != 0)
        }
        StakeStateV2::Initialized(meta) => {
            meta.authorized
                .check(&signers, StakeAuthorize::Withdrawer)?;
            // stake accounts must have a balance >= rent_exempt_reserve
            (meta.lockup, meta.rent_exempt_reserve, false)
        }
        StakeStateV2::Uninitialized => {
            if !signers.contains(stake_account.get_key()) {
                return Err(InstructionError::MissingRequiredSignature);
            }
            (Lockup::default(), 0, false) // no lockup, no restrictions
        }
        _ => return Err(InstructionError::InvalidAccountData),
    };

    // verify that lockup has expired or that the withdrawal is signed by
    //   the custodian, both epoch and unix_timestamp must have passed
    let custodian_pubkey = if let Some(custodian_index) = custodian_index {
        if instruction_context.is_instruction_account_signer(custodian_index)? {
            Some(
                transaction_context.get_key_of_account_at_index(
                    instruction_context
                        .get_index_of_instruction_account_in_transaction(custodian_index)?,
                )?,
            )
        } else {
            None
        }
    } else {
        None
    };
    if lockup.is_in_force(clock, custodian_pubkey) {
        return Err(StakeError::LockupInForce.into());
    }

    let lamports_and_reserve = checked_add(lamports, reserve)?;
    // if the stake is active, we mustn't allow the account to go away
    if is_staked // line coverage for branch coverage
            && lamports_and_reserve > stake_account.get_lamports()
    {
        return Err(InstructionError::InsufficientFunds);
    }

    if lamports != stake_account.get_lamports() // not a full withdrawal
            && lamports_and_reserve > stake_account.get_lamports()
    {
        assert!(!is_staked);
        return Err(InstructionError::InsufficientFunds);
    }

    // Deinitialize state upon zero balance
    if lamports == stake_account.get_lamports() {
        stake_account.set_state(&StakeStateV2::Uninitialized, feature_set)?;
    }

    stake_account.checked_sub_lamports(lamports, feature_set)?;
    drop(stake_account);
    let mut to =
        instruction_context.try_borrow_instruction_account(transaction_context, to_index)?;
    to.checked_add_lamports(lamports, feature_set)?;
    Ok(())
}

pub(crate) fn deactivate_delinquent(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account: &mut BorrowedAccount,
    delinquent_vote_account_index: IndexOfAccount,
    reference_vote_account_index: IndexOfAccount,
    current_epoch: Epoch,
) -> Result<(), InstructionError> {
    let delinquent_vote_account_pubkey = transaction_context.get_key_of_account_at_index(
        instruction_context
            .get_index_of_instruction_account_in_transaction(delinquent_vote_account_index)?,
    )?;
    let delinquent_vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, delinquent_vote_account_index)?;
    if *delinquent_vote_account.get_owner() != solana_vote_program::id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    let delinquent_vote_state = delinquent_vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    let reference_vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, reference_vote_account_index)?;
    if *reference_vote_account.get_owner() != solana_vote_program::id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    let reference_vote_state = reference_vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    if !acceptable_reference_epoch_credits(&reference_vote_state.epoch_credits, current_epoch) {
        return Err(StakeError::InsufficientReferenceVotes.into());
    }

    if let StakeStateV2::Stake(meta, mut stake, mut stake_flags) = stake_account.get_state()? {
        if stake.delegation.voter_pubkey != *delinquent_vote_account_pubkey {
            return Err(StakeError::VoteAddressMismatch.into());
        }

        // Deactivate the stake account if its delegated vote account has never voted or has not
        // voted in the last `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`
        if eligible_for_deactivate_delinquent(&delinquent_vote_state.epoch_credits, current_epoch) {
            deactivate_stake(invoke_context, &mut stake, &mut stake_flags, current_epoch)?;
            stake_account.set_state(
                &StakeStateV2::Stake(meta, stake, stake_flags),
                &invoke_context.feature_set,
            )
        } else {
            Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into())
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

/// After calling `validate_delegated_amount()`, this struct contains calculated values that are used
/// by the caller.
struct ValidatedDelegatedInfo {
    stake_amount: u64,
}

/// Ensure the stake delegation amount is valid.  This checks that the account meets the minimum
/// balance requirements of delegated stake.  If not, return an error.
fn validate_delegated_amount(
    account: &BorrowedAccount,
    meta: &Meta,
    feature_set: &FeatureSet,
) -> Result<ValidatedDelegatedInfo, InstructionError> {
    let stake_amount = account
        .get_lamports()
        .saturating_sub(meta.rent_exempt_reserve); // can't stake the rent

    // Stake accounts may be initialized with a stake amount below the minimum delegation so check
    // that the minimum is met before delegation.
    if stake_amount < crate::get_minimum_delegation(feature_set) {
        return Err(StakeError::InsufficientDelegation.into());
    }
    Ok(ValidatedDelegatedInfo { stake_amount })
}

/// After calling `validate_split_amount()`, this struct contains calculated values that are used
/// by the caller.
#[derive(Copy, Clone, Debug, Default)]
struct ValidatedSplitInfo {
    source_remaining_balance: u64,
    destination_rent_exempt_reserve: u64,
}

/// Ensure the split amount is valid.  This checks the source and destination accounts meet the
/// minimum balance requirements, which is the rent exempt reserve plus the minimum stake
/// delegation, and that the source account has enough lamports for the request split amount.  If
/// not, return an error.
fn validate_split_amount(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    source_account_index: IndexOfAccount,
    destination_account_index: IndexOfAccount,
    lamports: u64,
    source_meta: &Meta,
    additional_required_lamports: u64,
    source_is_active: bool,
) -> Result<ValidatedSplitInfo, InstructionError> {
    let source_account = instruction_context
        .try_borrow_instruction_account(transaction_context, source_account_index)?;
    let source_lamports = source_account.get_lamports();
    drop(source_account);
    let destination_account = instruction_context
        .try_borrow_instruction_account(transaction_context, destination_account_index)?;
    let destination_lamports = destination_account.get_lamports();
    let destination_data_len = destination_account.get_data().len();
    drop(destination_account);

    // Split amount has to be something
    if lamports == 0 {
        return Err(InstructionError::InsufficientFunds);
    }

    // Obviously cannot split more than what the source account has
    if lamports > source_lamports {
        return Err(InstructionError::InsufficientFunds);
    }

    // Verify that the source account still has enough lamports left after splitting:
    // EITHER at least the minimum balance, OR zero (in this case the source
    // account is transferring all lamports to new destination account, and the source
    // account will be closed)
    let source_minimum_balance = source_meta
        .rent_exempt_reserve
        .saturating_add(additional_required_lamports);
    let source_remaining_balance = source_lamports.saturating_sub(lamports);
    if source_remaining_balance == 0 {
        // full amount is a withdrawal
        // nothing to do here
    } else if source_remaining_balance < source_minimum_balance {
        // the remaining balance is too low to do the split
        return Err(InstructionError::InsufficientFunds);
    } else {
        // all clear!
        // nothing to do here
    }

    let rent = invoke_context.get_sysvar_cache().get_rent()?;
    let destination_rent_exempt_reserve = rent.minimum_balance(destination_data_len);

    // As of feature `require_rent_exempt_split_destination`, if the source is active stake, one of
    // these criteria must be met:
    // 1. the destination account must be prefunded with at least the rent-exempt reserve, or
    // 2. the split must consume 100% of the source
    if invoke_context
        .feature_set
        .is_active(&feature_set::require_rent_exempt_split_destination::id())
        && source_is_active
        && source_remaining_balance != 0
        && destination_lamports < destination_rent_exempt_reserve
    {
        return Err(InstructionError::InsufficientFunds);
    }

    // Verify the destination account meets the minimum balance requirements
    // This must handle:
    // 1. The destination account having a different rent exempt reserve due to data size changes
    // 2. The destination account being prefunded, which would lower the minimum split amount
    let destination_minimum_balance =
        destination_rent_exempt_reserve.saturating_add(additional_required_lamports);
    let destination_balance_deficit =
        destination_minimum_balance.saturating_sub(destination_lamports);
    if lamports < destination_balance_deficit {
        return Err(InstructionError::InsufficientFunds);
    }

    Ok(ValidatedSplitInfo {
        source_remaining_balance,
        destination_rent_exempt_reserve,
    })
}

#[derive(Clone, Debug, PartialEq)]
enum MergeKind {
    Inactive(Meta, u64, StakeFlags),
    ActivationEpoch(Meta, Stake, StakeFlags),
    FullyActive(Meta, Stake),
}

impl MergeKind {
    fn meta(&self) -> &Meta {
        match self {
            Self::Inactive(meta, _, _) => meta,
            Self::ActivationEpoch(meta, _, _) => meta,
            Self::FullyActive(meta, _) => meta,
        }
    }

    fn active_stake(&self) -> Option<&Stake> {
        match self {
            Self::Inactive(_, _, _) => None,
            Self::ActivationEpoch(_, stake, _) => Some(stake),
            Self::FullyActive(_, stake) => Some(stake),
        }
    }

    fn get_if_mergeable(
        invoke_context: &InvokeContext,
        stake_state: &StakeStateV2,
        stake_lamports: u64,
        clock: &Clock,
        stake_history: &StakeHistory,
    ) -> Result<Self, InstructionError> {
        match stake_state {
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                // stake must not be in a transient state. Transient here meaning
                // activating or deactivating with non-zero effective stake.
                let status = stake.delegation.stake_activating_and_deactivating(
                    clock.epoch,
                    stake_history,
                    new_warmup_cooldown_rate_epoch(invoke_context),
                );

                match (status.effective, status.activating, status.deactivating) {
                    (0, 0, 0) => Ok(Self::Inactive(*meta, stake_lamports, *stake_flags)),
                    (0, _, _) => Ok(Self::ActivationEpoch(*meta, *stake, *stake_flags)),
                    (_, 0, 0) => Ok(Self::FullyActive(*meta, *stake)),
                    _ => {
                        let err = StakeError::MergeTransientStake;
                        ic_msg!(invoke_context, "{}", err);
                        Err(err.into())
                    }
                }
            }
            StakeStateV2::Initialized(meta) => {
                Ok(Self::Inactive(*meta, stake_lamports, StakeFlags::empty()))
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }

    fn metas_can_merge(
        invoke_context: &InvokeContext,
        stake: &Meta,
        source: &Meta,
        clock: &Clock,
    ) -> Result<(), InstructionError> {
        // lockups may mismatch so long as both have expired
        let can_merge_lockups = stake.lockup == source.lockup
            || (!stake.lockup.is_in_force(clock, None) && !source.lockup.is_in_force(clock, None));
        // `rent_exempt_reserve` has no bearing on the mergeability of accounts,
        // as the source account will be culled by runtime once the operation
        // succeeds. Considering it here would needlessly prevent merging stake
        // accounts with differing data lengths, which already exist in the wild
        // due to an SDK bug
        if stake.authorized == source.authorized && can_merge_lockups {
            Ok(())
        } else {
            ic_msg!(invoke_context, "Unable to merge due to metadata mismatch");
            Err(StakeError::MergeMismatch.into())
        }
    }

    fn active_delegations_can_merge(
        invoke_context: &InvokeContext,
        stake: &Delegation,
        source: &Delegation,
    ) -> Result<(), InstructionError> {
        if stake.voter_pubkey != source.voter_pubkey {
            ic_msg!(invoke_context, "Unable to merge due to voter mismatch");
            Err(StakeError::MergeMismatch.into())
        } else if stake.deactivation_epoch == Epoch::MAX && source.deactivation_epoch == Epoch::MAX
        {
            Ok(())
        } else {
            ic_msg!(invoke_context, "Unable to merge due to stake deactivation");
            Err(StakeError::MergeMismatch.into())
        }
    }

    fn merge(
        self,
        invoke_context: &InvokeContext,
        source: Self,
        clock: &Clock,
    ) -> Result<Option<StakeStateV2>, InstructionError> {
        Self::metas_can_merge(invoke_context, self.meta(), source.meta(), clock)?;
        self.active_stake()
            .zip(source.active_stake())
            .map(|(stake, source)| {
                Self::active_delegations_can_merge(
                    invoke_context,
                    &stake.delegation,
                    &source.delegation,
                )
            })
            .unwrap_or(Ok(()))?;
        let merged_state = match (self, source) {
            (Self::Inactive(_, _, _), Self::Inactive(_, _, _)) => None,
            (Self::Inactive(_, _, _), Self::ActivationEpoch(_, _, _)) => None,
            (
                Self::ActivationEpoch(meta, mut stake, stake_flags),
                Self::Inactive(_, source_lamports, source_stake_flags),
            ) => {
                stake.delegation.stake = checked_add(stake.delegation.stake, source_lamports)?;
                Some(StakeStateV2::Stake(
                    meta,
                    stake,
                    stake_flags.union(source_stake_flags),
                ))
            }
            (
                Self::ActivationEpoch(meta, mut stake, stake_flags),
                Self::ActivationEpoch(source_meta, source_stake, source_stake_flags),
            ) => {
                let source_lamports = checked_add(
                    source_meta.rent_exempt_reserve,
                    source_stake.delegation.stake,
                )?;
                merge_delegation_stake_and_credits_observed(
                    &mut stake,
                    source_lamports,
                    source_stake.credits_observed,
                )?;
                Some(StakeStateV2::Stake(
                    meta,
                    stake,
                    stake_flags.union(source_stake_flags),
                ))
            }
            (Self::FullyActive(meta, mut stake), Self::FullyActive(_, source_stake)) => {
                // Don't stake the source account's `rent_exempt_reserve` to
                // protect against the magic activation loophole. It will
                // instead be moved into the destination account as extra,
                // withdrawable `lamports`
                merge_delegation_stake_and_credits_observed(
                    &mut stake,
                    source_stake.delegation.stake,
                    source_stake.credits_observed,
                )?;
                Some(StakeStateV2::Stake(meta, stake, StakeFlags::empty()))
            }
            _ => return Err(StakeError::MergeMismatch.into()),
        };
        Ok(merged_state)
    }
}

fn merge_delegation_stake_and_credits_observed(
    stake: &mut Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Result<(), InstructionError> {
    stake.credits_observed =
        stake_weighted_credits_observed(stake, absorbed_lamports, absorbed_credits_observed)
            .ok_or(InstructionError::ArithmeticOverflow)?;
    stake.delegation.stake = checked_add(stake.delegation.stake, absorbed_lamports)?;
    Ok(())
}

/// Calculate the effective credits observed for two stakes when merging
///
/// When merging two `ActivationEpoch` or `FullyActive` stakes, the credits
/// observed of the merged stake is the weighted average of the two stakes'
/// credits observed.
///
/// This is because we can derive the effective credits_observed by reversing the staking
/// rewards equation, _while keeping the rewards unchanged after merge (i.e. strong
/// requirement)_, like below:
///
/// a(N) => account, r => rewards, s => stake, c => credits:
/// assume:
///   a3 = merge(a1, a2)
/// then:
///   a3.s = a1.s + a2.s
///
/// Next, given:
///   aN.r = aN.c * aN.s (for every N)
/// finally:
///        a3.r = a1.r + a2.r
/// a3.c * a3.s = a1.c * a1.s + a2.c * a2.s
///        a3.c = (a1.c * a1.s + a2.c * a2.s) / (a1.s + a2.s)     // QED
///
/// (For this discussion, we omitted irrelevant variables, including distance
///  calculation against vote_account and point indirection.)
fn stake_weighted_credits_observed(
    stake: &Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Option<u64> {
    if stake.credits_observed == absorbed_credits_observed {
        Some(stake.credits_observed)
    } else {
        let total_stake = u128::from(stake.delegation.stake.checked_add(absorbed_lamports)?);
        let stake_weighted_credits =
            u128::from(stake.credits_observed).checked_mul(u128::from(stake.delegation.stake))?;
        let absorbed_weighted_credits =
            u128::from(absorbed_credits_observed).checked_mul(u128::from(absorbed_lamports))?;
        // Discard fractional credits as a merge side-effect friction by taking
        // the ceiling, done by adding `denominator - 1` to the numerator.
        let total_weighted_credits = stake_weighted_credits
            .checked_add(absorbed_weighted_credits)?
            .checked_add(total_stake)?
            .checked_sub(1)?;
        u64::try_from(total_weighted_credits.checked_div(total_stake)?).ok()
    }
}

// utility function, used by runtime
// returns a tuple of (stakers_reward,voters_reward)
#[doc(hidden)]
pub fn redeem_rewards(
    rewarded_epoch: Epoch,
    stake_state: StakeStateV2,
    stake_account: &mut AccountSharedData,
    vote_state: &VoteState,
    point_value: &PointValue,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> Result<(u64, u64), InstructionError> {
    if let StakeStateV2::Stake(meta, mut stake, stake_flags) = stake_state {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(
                &InflationPointCalculationEvent::EffectiveStakeAtRewardedEpoch(stake.stake(
                    rewarded_epoch,
                    stake_history,
                    new_rate_activation_epoch,
                )),
            );
            inflation_point_calc_tracer(&InflationPointCalculationEvent::RentExemptReserve(
                meta.rent_exempt_reserve,
            ));
            inflation_point_calc_tracer(&InflationPointCalculationEvent::Commission(
                vote_state.commission,
            ));
        }

        if let Some((stakers_reward, voters_reward)) = redeem_stake_rewards(
            rewarded_epoch,
            &mut stake,
            point_value,
            vote_state,
            stake_history,
            inflation_point_calc_tracer,
            new_rate_activation_epoch,
        ) {
            stake_account.checked_add_lamports(stakers_reward)?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))?;

            Ok((stakers_reward, voters_reward))
        } else {
            Err(StakeError::NoCreditsToRedeem.into())
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

// utility function, used by runtime
#[doc(hidden)]
pub fn calculate_points(
    stake_state: &StakeStateV2,
    vote_state: &VoteState,
    stake_history: &StakeHistory,
    new_rate_activation_epoch: Option<Epoch>,
) -> Result<u128, InstructionError> {
    if let StakeStateV2::Stake(_meta, stake, _stake_flags) = stake_state {
        Ok(calculate_stake_points(
            stake,
            vote_state,
            stake_history,
            null_tracer(),
            new_rate_activation_epoch,
        ))
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

pub type RewriteStakeStatus = (&'static str, (u64, u64), (u64, u64));

// utility function, used by runtime::Stakes, tests
pub fn new_stake_history_entry<'a, I>(
    epoch: Epoch,
    stakes: I,
    history: &StakeHistory,
    new_rate_activation_epoch: Option<Epoch>,
) -> StakeHistoryEntry
where
    I: Iterator<Item = &'a Delegation>,
{
    stakes.fold(StakeHistoryEntry::default(), |sum, stake| {
        sum + stake.stake_activating_and_deactivating(epoch, history, new_rate_activation_epoch)
    })
}

// utility function, used by tests
pub fn create_stake_history_from_delegations(
    bootstrap: Option<u64>,
    epochs: std::ops::Range<Epoch>,
    delegations: &[Delegation],
    new_rate_activation_epoch: Option<Epoch>,
) -> StakeHistory {
    let mut stake_history = StakeHistory::default();

    let bootstrap_delegation = if let Some(bootstrap) = bootstrap {
        vec![Delegation {
            activation_epoch: std::u64::MAX,
            stake: bootstrap,
            ..Delegation::default()
        }]
    } else {
        vec![]
    };

    for epoch in epochs {
        let entry = new_stake_history_entry(
            epoch,
            delegations.iter().chain(bootstrap_delegation.iter()),
            &stake_history,
            new_rate_activation_epoch,
        );
        stake_history.add(epoch, entry);
    }

    stake_history
}

// genesis investor accounts
pub fn create_lockup_stake_account(
    authorized: &Authorized,
    lockup: &Lockup,
    rent: &Rent,
    lamports: u64,
) -> AccountSharedData {
    let mut stake_account = AccountSharedData::new(lamports, StakeStateV2::size_of(), &id());

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data().len());
    assert!(
        lamports >= rent_exempt_reserve,
        "lamports: {lamports} is less than rent_exempt_reserve {rent_exempt_reserve}"
    );

    stake_account
        .set_state(&StakeStateV2::Initialized(Meta {
            authorized: *authorized,
            lockup: *lockup,
            rent_exempt_reserve,
        }))
        .expect("set_state");

    stake_account
}

// utility function, used by Bank, tests, genesis for bootstrap
pub fn create_account(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
) -> AccountSharedData {
    do_create_account(
        authorized,
        voter_pubkey,
        vote_account,
        rent,
        lamports,
        Epoch::MAX,
    )
}

// utility function, used by tests
pub fn create_account_with_activation_epoch(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
    activation_epoch: Epoch,
) -> AccountSharedData {
    do_create_account(
        authorized,
        voter_pubkey,
        vote_account,
        rent,
        lamports,
        activation_epoch,
    )
}

fn do_create_account(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
    activation_epoch: Epoch,
) -> AccountSharedData {
    let mut stake_account = AccountSharedData::new(lamports, StakeStateV2::size_of(), &id());

    let vote_state = vote_state::from(vote_account).expect("vote_state");

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data().len());

    stake_account
        .set_state(&StakeStateV2::Stake(
            Meta {
                authorized: Authorized::auto(authorized),
                rent_exempt_reserve,
                ..Meta::default()
            },
            new_stake(
                lamports - rent_exempt_reserve, // underflow is an error, is basically: assert!(lamports > rent_exempt_reserve);
                voter_pubkey,
                &vote_state,
                activation_epoch,
            ),
            StakeFlags::empty(),
        ))
        .expect("set_state");

    stake_account
}
