#![allow(dead_code)]
#![allow(unused_imports)]

use {
    crate::{feature_set_die, id, stake_history_die},
    num_traits::cast::ToPrimitive,
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::{Clock, Epoch},
        entrypoint::ProgramResult,
        instruction::InstructionError,
        msg,
        program_error::ProgramError,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        rent::Rent,
        stake::state::*,
        stake::{
            instruction::{LockupArgs, LockupCheckedArgs, StakeError, StakeInstruction},
            stake_flags::StakeFlags,
            state::{Authorized, Lockup},
            tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
        },
        stake_history::{StakeHistoryData, StakeHistoryEntry},
        sysvar::Sysvar,
        vote::program as solana_vote_program,
        vote::state::{VoteState, VoteStateVersions},
    },
    std::{cmp::Ordering, collections::HashSet, convert::TryFrom},
};

// XXX note to self. InstructionError is actually a superset of ProgramError
// there is a TryFrom instance, but thats why theres no From instance
// there are ProgramError conversions between u64 tho, and From<T> for InstructionError where T: FromPrimitive
// very unusual. i guess i can look more into this but for now using ProgramError is fine seems safe

// XXX a nice change would be to pop an account off the queue and discard if its a gettable sysvar
// ie, allow people to omit them from the accounts list without breaking compat

/// XXX THIS SECTION is new utility functions and stuff like that

// XXX check for more efficient parser
fn get_stake_state(stake_account_info: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if *stake_account_info.owner != id() {
        return Err(ProgramError::InvalidAccountOwner);
    }

    stake_account_info
        .deserialize_data()
        .map_err(|_| ProgramError::InvalidAccountData)
}

// XXX errors changed from GenericError
fn set_stake_state(stake_account_info: &AccountInfo, new_state: &StakeStateV2) -> ProgramResult {
    let serialized_size =
        bincode::serialized_size(new_state).map_err(|_| ProgramError::InvalidAccountData)?;
    if serialized_size > stake_account_info.data_len() as u64 {
        return Err(ProgramError::AccountDataTooSmall);
    }

    bincode::serialize_into(&mut stake_account_info.data.borrow_mut()[..], new_state)
        .map_err(|_| ProgramError::InvalidAccountData)
}

fn collect_signers(
    account_infos: &[&AccountInfo],
    checked: bool,
) -> Result<HashSet<Pubkey>, ProgramError> {
    let mut signers = HashSet::new();

    for account_info in account_infos {
        if account_info.is_signer {
            signers.insert(*account_info.key);
        } else if checked {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    Ok(signers)
}

pub fn checked_add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(ProgramError::InsufficientFunds)
}

// XXX impl from<StakeError> for ProgramError. also idk if this is correct
// i just want to keep the same errors in-place and then clean up later, instead of needing to hunt down the right ones
// XXX there should also be a better wrapper for TryFrom<InstructionError> for ProgramError
// like, if theres a matching error do the conversion, if custom do the custom conversion
// otherwise unwrap into an error cnoversion error maybe. idk
pub trait TurnInto {
    fn turn_into(self) -> ProgramError;
}
impl TurnInto for StakeError {
    fn turn_into(self) -> ProgramError {
        ProgramError::Custom(self.to_u32().unwrap())
    }
}
impl TurnInto for InstructionError {
    fn turn_into(self) -> ProgramError {
        match ProgramError::try_from(self) {
            Ok(program_error) => program_error,
            Err(e) => panic!("HANA error conversion failed: {:?}", e),
        }
    }
}

/// XXX THIS SECTION is mostly copy-pasted from stake_state.rs

/// After calling `validate_delegated_amount()`, this struct contains calculated values that are used
/// by the caller.
struct ValidatedDelegatedInfo {
    stake_amount: u64,
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

// XXX this is stubbed out because it depends on invoke context and feature set
// what this does is calls a feature_set.rs function also called `new_warmup_cooldown_rate_epoch`
// which gets the slot that the `reduce_stake_warmup_cooldown` feature was activated at
// and then passes the slot to `epoch_schedule.get_epoch()` to convert it to an epoch
// in other words `new_warmup_cooldown_rate_epoch` does exactly what it says
// this results in a Option<Epoch> that gets passed into various stake functions
// get activating/deactivating, calculate rewards, etc
//
// ok so that means if the feature isnt active we return None. easy
// if the feature *is* active then its tricky if we dont have access to the featureset
// EpochSchedule has a sysvar get impl but we would need to... hardcode the epochs for the networks? idk
//
// TODO i need to look at wtf this number is actually used for
// presumbly it is not as simple as just "are we active yet" otherwise there wouldnt be this dance
// i assume the intent is stake history behaves differently before and after the cutover
// but i *believe* all this stuff is to change it from "we have a 25% deactivation cap defined by stake config"
// to "we have a 7% deactivation cap hardcoded" so we could deploy a second feature to get rid of the plumbing
// once history has an epoch in it when there is less than 7% deactivation? idk
// history is fucking confusing to me still
// maybe i should write a post about it and have someone just factcheck me so i understand lol
pub(crate) fn new_warmup_cooldown_rate_epoch() -> Option<Epoch> {
    None
}

/// Ensure the stake delegation amount is valid.  This checks that the account meets the minimum
/// balance requirements of delegated stake.  If not, return an error.
fn validate_delegated_amount(
    account: &AccountInfo,
    meta: &Meta,
) -> Result<ValidatedDelegatedInfo, ProgramError> {
    let stake_amount = account.lamports().saturating_sub(meta.rent_exempt_reserve); // can't stake the rent

    // Stake accounts may be initialized with a stake amount below the minimum delegation so check
    // that the minimum is met before delegation.
    if stake_amount < crate::get_minimum_delegation() {
        return Err(StakeError::InsufficientDelegation.turn_into());
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
    source_lamports: u64,
    destination_lamports: u64,
    split_lamports: u64,
    source_meta: &Meta,
    destination_data_len: usize,
    additional_required_lamports: u64,
    source_is_active: bool,
) -> Result<ValidatedSplitInfo, ProgramError> {
    // Split amount has to be something
    if split_lamports == 0 {
        return Err(ProgramError::InsufficientFunds);
    }

    // Obviously cannot split more than what the source account has
    if split_lamports > source_lamports {
        return Err(ProgramError::InsufficientFunds);
    }

    // Verify that the source account still has enough lamports left after splitting:
    // EITHER at least the minimum balance, OR zero (in this case the source
    // account is transferring all lamports to new destination account, and the source
    // account will be closed)
    let source_minimum_balance = source_meta
        .rent_exempt_reserve
        .saturating_add(additional_required_lamports);
    let source_remaining_balance = source_lamports.saturating_sub(split_lamports);
    if source_remaining_balance == 0 {
        // full amount is a withdrawal
        // nothing to do here
    } else if source_remaining_balance < source_minimum_balance {
        // the remaining balance is too low to do the split
        return Err(ProgramError::InsufficientFunds);
    } else {
        // all clear!
        // nothing to do here
    }

    let rent = Rent::get()?;
    let destination_rent_exempt_reserve = rent.minimum_balance(destination_data_len);

    // As of feature `require_rent_exempt_split_destination`, if the source is active stake, one of
    // these criteria must be met:
    // 1. the destination account must be prefunded with at least the rent-exempt reserve, or
    // 2. the split must consume 100% of the source
    if crate::FEATURE_REQUIRE_RENT_EXEMPT_SPLIT_DESTINATION
        && source_is_active
        && source_remaining_balance != 0
        && destination_lamports < destination_rent_exempt_reserve
    {
        return Err(ProgramError::InsufficientFunds);
    }

    // Verify the destination account meets the minimum balance requirements
    // This must handle:
    // 1. The destination account having a different rent exempt reserve due to data size changes
    // 2. The destination account being prefunded, which would lower the minimum split amount
    let destination_minimum_balance =
        destination_rent_exempt_reserve.saturating_add(additional_required_lamports);
    let destination_balance_deficit =
        destination_minimum_balance.saturating_sub(destination_lamports);
    if split_lamports < destination_balance_deficit {
        return Err(ProgramError::InsufficientFunds);
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
        stake_state: &StakeStateV2,
        stake_lamports: u64,
        clock: &Clock,
        stake_history: &StakeHistoryData,
    ) -> Result<Self, ProgramError> {
        match stake_state {
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                // stake must not be in a transient state. Transient here meaning
                // activating or deactivating with non-zero effective stake.
                let status = stake.delegation.stake_activating_and_deactivating(
                    clock.epoch,
                    stake_history,
                    new_warmup_cooldown_rate_epoch(),
                );

                match (status.effective, status.activating, status.deactivating) {
                    (0, 0, 0) => Ok(Self::Inactive(*meta, stake_lamports, *stake_flags)),
                    (0, _, _) => Ok(Self::ActivationEpoch(*meta, *stake, *stake_flags)),
                    (_, 0, 0) => Ok(Self::FullyActive(*meta, *stake)),
                    _ => {
                        let err = StakeError::MergeTransientStake;
                        msg!("{}", err);
                        Err(err.turn_into())
                    }
                }
            }
            StakeStateV2::Initialized(meta) => {
                Ok(Self::Inactive(*meta, stake_lamports, StakeFlags::empty()))
            }
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    fn metas_can_merge(stake: &Meta, source: &Meta, clock: &Clock) -> ProgramResult {
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
            msg!("Unable to merge due to metadata mismatch");
            Err(StakeError::MergeMismatch.turn_into())
        }
    }

    fn active_delegations_can_merge(stake: &Delegation, source: &Delegation) -> ProgramResult {
        if stake.voter_pubkey != source.voter_pubkey {
            msg!("Unable to merge due to voter mismatch");
            Err(StakeError::MergeMismatch.turn_into())
        } else if stake.deactivation_epoch == Epoch::MAX && source.deactivation_epoch == Epoch::MAX
        {
            Ok(())
        } else {
            msg!("Unable to merge due to stake deactivation");
            Err(StakeError::MergeMismatch.turn_into())
        }
    }

    fn merge(self, source: Self, clock: &Clock) -> Result<Option<StakeStateV2>, ProgramError> {
        Self::metas_can_merge(self.meta(), source.meta(), clock)?;
        self.active_stake()
            .zip(source.active_stake())
            .map(|(stake, source)| {
                Self::active_delegations_can_merge(&stake.delegation, &source.delegation)
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
            _ => return Err(StakeError::MergeMismatch.turn_into()),
        };
        Ok(merged_state)
    }
}

fn merge_delegation_stake_and_credits_observed(
    stake: &mut Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> ProgramResult {
    stake.credits_observed =
        stake_weighted_credits_observed(stake, absorbed_lamports, absorbed_credits_observed)
            .ok_or(ProgramError::ArithmeticOverflow)?;
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

/// XXX THIS SECTION is the new processor

fn do_authorize(
    stake_account_info: &AccountInfo,
    signers: &HashSet<Pubkey>,
    new_authority: &Pubkey,
    authority_type: StakeAuthorize,
    custodian: Option<&Pubkey>,
    clock: &Clock,
) -> ProgramResult {
    match get_stake_state(stake_account_info)? {
        StakeStateV2::Initialized(mut meta) => {
            meta.authorized
                .authorize(
                    &signers,
                    new_authority,
                    authority_type,
                    Some((&meta.lockup, clock, custodian)),
                )
                .map_err(InstructionError::turn_into)?;

            set_stake_state(stake_account_info, &StakeStateV2::Initialized(meta))
        }
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.authorized
                .authorize(
                    &signers,
                    new_authority,
                    authority_type,
                    Some((&meta.lockup, clock, custodian)),
                )
                .map_err(InstructionError::turn_into)?;

            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, stake_flags),
            )
        }
        _ => Err(ProgramError::InvalidAccountData),
    }
}

fn do_set_lockup(
    stake_account_info: &AccountInfo,
    signers: HashSet<Pubkey>,
    lockup: &LockupArgs,
    clock: &Clock,
) -> ProgramResult {
    match get_stake_state(stake_account_info)? {
        StakeStateV2::Initialized(mut meta) => {
            meta.set_lockup(lockup, &signers, clock)
                .map_err(InstructionError::turn_into)?;

            set_stake_state(stake_account_info, &StakeStateV2::Initialized(meta))
        }
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.set_lockup(lockup, &signers, clock)
                .map_err(InstructionError::turn_into)?;

            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, stake_flags),
            )
        }
        _ => Err(ProgramError::InvalidAccountData),
    }
}

pub struct Processor {}
impl Processor {
    fn process_initialize(
        accounts: &[AccountInfo],
        authorized: Authorized,
        lockup: Lockup,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let rent_info = next_account_info(account_info_iter)?;
        let rent = &Rent::from_account_info(rent_info)?;

        if stake_account_info.data_len() != StakeStateV2::size_of() {
            return Err(ProgramError::InvalidAccountData);
        }

        if let StakeStateV2::Uninitialized = get_stake_state(stake_account_info)? {
            let rent_exempt_reserve = rent.minimum_balance(stake_account_info.data_len());
            if stake_account_info.lamports() >= rent_exempt_reserve {
                let stake_state = StakeStateV2::Initialized(Meta {
                    rent_exempt_reserve,
                    authorized,
                    lockup,
                });

                set_stake_state(stake_account_info, &stake_state)?;

                Ok(()) // XXX the above error as-written is InstructionError::GenericError
            } else {
                Err(ProgramError::InsufficientFunds)
            }
        } else {
            Err(ProgramError::InvalidAccountData)
        }?;

        Ok(())
    }

    fn process_authorize(
        accounts: &[AccountInfo],
        new_authority: Pubkey,
        authority_type: StakeAuthorize,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let (signers, custodian) = if let Some(lockup_authority_info) = option_lockup_authority_info
        {
            (
                collect_signers(
                    &[stake_or_withdraw_authority_info, lockup_authority_info],
                    false,
                )?,
                Some(lockup_authority_info.key),
            )
        } else {
            (
                collect_signers(&[stake_or_withdraw_authority_info], false)?,
                None,
            )
        };

        do_authorize(
            stake_account_info,
            &signers,
            &new_authority,
            authority_type,
            custodian,
            clock,
        )?;

        Ok(())
    }

    fn process_authorize_checked(
        accounts: &[AccountInfo],
        authority_type: StakeAuthorize,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let old_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
        let new_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let (signers, custodian) = if let Some(lockup_authority_info) = option_lockup_authority_info
        {
            (
                collect_signers(
                    &[
                        old_stake_or_withdraw_authority_info,
                        new_stake_or_withdraw_authority_info,
                        lockup_authority_info,
                    ],
                    true,
                )?,
                Some(lockup_authority_info.key),
            )
        } else {
            (
                collect_signers(
                    &[
                        old_stake_or_withdraw_authority_info,
                        new_stake_or_withdraw_authority_info,
                    ],
                    true,
                )?,
                None,
            )
        };

        do_authorize(
            stake_account_info,
            &signers,
            new_stake_or_withdraw_authority_info.key,
            authority_type,
            custodian,
            clock,
        )?;

        Ok(())
    }

    fn process_delegate(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let vote_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let _stake_history_info = next_account_info(account_info_iter)?;
        let _stake_config_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        if *vote_account_info.owner != solana_vote_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        let signers = collect_signers(&[stake_authority_info], false)?;

        // XXX when im back on a branch with this
        //let mut vote_state = Box::new(VoteState::default());
        //VoteState::deserialize_into(&vote_account_info.data.borrow(), &mut vote_state).unwrap();
        //let vote_state = vote_state;
        let vote_state = VoteState::deserialize(&vote_account_info.data.borrow()).unwrap();

        match get_stake_state(stake_account_info)? {
            StakeStateV2::Initialized(meta) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(InstructionError::turn_into)?;

                let ValidatedDelegatedInfo { stake_amount } =
                    validate_delegated_amount(&stake_account_info, &meta)?;

                let new_stake_state = new_stake(
                    stake_amount,
                    vote_account_info.key,
                    &vote_state,
                    clock.epoch,
                );

                set_stake_state(
                    stake_account_info,
                    &StakeStateV2::Stake(meta, new_stake_state, StakeFlags::empty()),
                )
            }
            _ => Err(ProgramError::InvalidAccountData),
        }?;

        Ok(())
    }

    fn process_deactivate(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        let signers = collect_signers(&[stake_authority_info], false)?;

        match get_stake_state(stake_account_info)? {
            StakeStateV2::Stake(meta, mut stake, stake_flags) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(InstructionError::turn_into)?;

                stake
                    .deactivate(clock.epoch)
                    .map_err(StakeError::turn_into)?;

                set_stake_state(
                    stake_account_info,
                    &StakeStateV2::Stake(meta, stake, stake_flags),
                )
            }
            _ => Err(ProgramError::InvalidAccountData),
        }?;

        Ok(())
    }

    fn process_set_lockup(accounts: &[AccountInfo], lockup: LockupArgs) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let old_withdraw_or_lockup_authority_info = next_account_info(account_info_iter)?;
        let clock = Clock::get()?;

        let signers = collect_signers(&[old_withdraw_or_lockup_authority_info], false)?;

        do_set_lockup(stake_account_info, signers, &lockup, &clock)?;

        Ok(())
    }

    fn process_set_lockup_checked(
        accounts: &[AccountInfo],
        lockup_checked: LockupCheckedArgs,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let old_withdraw_or_lockup_authority_info = next_account_info(account_info_iter)?;
        let option_new_lockup_authority_info = next_account_info(account_info_iter).ok();
        let clock = Clock::get()?;

        let (signers, custodian) =
            if let Some(new_lockup_authority_info) = option_new_lockup_authority_info {
                (
                    collect_signers(
                        &[
                            old_withdraw_or_lockup_authority_info,
                            new_lockup_authority_info,
                        ],
                        true,
                    )?,
                    Some(*new_lockup_authority_info.key),
                )
            } else {
                (
                    collect_signers(&[old_withdraw_or_lockup_authority_info], true)?,
                    None,
                )
            };

        let lockup = LockupArgs {
            unix_timestamp: lockup_checked.unix_timestamp,
            epoch: lockup_checked.epoch,
            custodian,
        };

        do_set_lockup(stake_account_info, signers, &lockup, &clock)?;

        Ok(())
    }

    fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let stake_history_info = next_account_info(account_info_iter)?;
        let stake_history = &StakeHistoryData::from_account_info(stake_history_info)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        if source_stake_account_info.key == destination_stake_account_info.key {
            return Err(ProgramError::InvalidArgument);
        }

        // XXX TODO FIXME this replicates the behavior of the existing program but probably better to check
        // do it after i can test this program lol
        let signers = collect_signers(&[stake_authority_info], false)?;

        msg!("Checking if destination stake is mergeable");
        let destination_merge_kind = MergeKind::get_if_mergeable(
            &get_stake_state(destination_stake_account_info)?,
            destination_stake_account_info.lamports(),
            clock,
            stake_history,
        )?;

        // Authorized staker is allowed to split/merge accounts
        destination_merge_kind
            .meta()
            .authorized
            .check(&signers, StakeAuthorize::Staker)
            .map_err(|_| ProgramError::MissingRequiredSignature)?;

        msg!("Checking if source stake is mergeable");
        let source_merge_kind = MergeKind::get_if_mergeable(
            &get_stake_state(source_stake_account_info)?,
            source_stake_account_info.lamports(),
            clock,
            stake_history,
        )?;

        msg!("Merging stake accounts");
        if let Some(merged_state) = destination_merge_kind.merge(source_merge_kind, clock)? {
            set_stake_state(destination_stake_account_info, &merged_state)?;
        }

        // Source is about to be drained, deinitialize its state
        set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;

        // Drain the source stake account
        let lamports = source_stake_account_info.lamports();

        // XXX are there nicer helpers for AccountInfo? checked_{add,sub}_lamports dont exist
        let mut source_lamports = source_stake_account_info.try_borrow_mut_lamports()?;
        **source_lamports = source_lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::InsufficientFunds)?;

        let mut destination_lamports = destination_stake_account_info.try_borrow_mut_lamports()?;
        **destination_lamports = destination_lamports
            .checked_add(lamports)
            .ok_or(ProgramError::InsufficientFunds)?;

        Ok(())
    }

    fn process_split(accounts: &[AccountInfo], split_lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        // XXX TODO FIXME this replicates the behavior of the existing program but probably better to check
        let signers = collect_signers(&[stake_authority_info], false)?;

        let destination_data_len = destination_stake_account_info.data_len();
        if destination_data_len != StakeStateV2::size_of() {
            return Err(ProgramError::InvalidAccountData);
        }

        if let StakeStateV2::Uninitialized = get_stake_state(destination_stake_account_info)? {
            // we can split into this
        } else {
            return Err(ProgramError::InvalidAccountData);
        }

        let source_lamport_balance = source_stake_account_info.lamports();
        let destination_lamport_balance = destination_stake_account_info.lamports();

        if split_lamports > source_lamport_balance {
            return Err(ProgramError::InsufficientFunds);
        }

        match get_stake_state(source_stake_account_info)? {
            StakeStateV2::Stake(source_meta, mut source_stake, stake_flags) => {
                source_meta
                    .authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(InstructionError::turn_into)?;

                let minimum_delegation = crate::get_minimum_delegation();

                // XXX TODO FIXME get_stake_status turns out... to require stake history which isnt passed in as an account
                // all of my wonderful plans are laid to waste. this is going to cause problems
                // XXX ok maybe i can work around this. all we are trying to do is see if there is any effective stake at all
                // i will need to actually understand stake history for this but
                // it is possible we can just be more conservative. and say "if it looks like it might be active, its active"
                // i think we can. this can *only* return false if we are gte our activation epoch but nothing is active yet
                // which... basically never happens. and then this value is used in validate_split_amount
                // which is like. "if feature is active and source is active and not splitting 100%
                // *and* destination is below the reserve requirement then error
                // which means without stake history we just say... wait thats unfortunately not true
                // because a *deactivated* stake could never be split... and there are plausible usecases for that
                // i think we are fucked. unless we just say. you have to pass in stake history if splitting deactive
                let is_active = if crate::FEATURE_REQUIRE_RENT_EXEMPT_SPLIT_DESTINATION {
                    let clock = Clock::get()?;
                    let stake_history = &StakeHistoryData::default(); // FIXME
                    let new_rate_activation_epoch = new_warmup_cooldown_rate_epoch();

                    let status = source_stake.delegation.stake_activating_and_deactivating(
                        clock.epoch,
                        stake_history,
                        new_rate_activation_epoch,
                    );

                    status.effective > 0
                } else {
                    false
                };

                // XXX note this function also internally summons Rent via syscall
                let validated_split_info = validate_split_amount(
                    source_lamport_balance,
                    destination_lamport_balance,
                    split_lamports,
                    &source_meta,
                    destination_data_len,
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
                        let remaining_stake_delta =
                            split_lamports.saturating_sub(source_meta.rent_exempt_reserve);
                        (remaining_stake_delta, remaining_stake_delta)
                    } else {
                        // Otherwise, the new split stake should reflect the entire split
                        // requested, less any lamports needed to cover the split_rent_exempt_reserve.
                        if source_stake.delegation.stake.saturating_sub(split_lamports)
                            < minimum_delegation
                        {
                            return Err(StakeError::InsufficientDelegation.turn_into());
                        }

                        (
                            split_lamports,
                            split_lamports.saturating_sub(
                                validated_split_info
                                    .destination_rent_exempt_reserve
                                    .saturating_sub(destination_lamport_balance),
                            ),
                        )
                    };

                if split_stake_amount < minimum_delegation {
                    return Err(StakeError::InsufficientDelegation.turn_into());
                }

                let destination_stake = source_stake
                    .split(remaining_stake_delta, split_stake_amount)
                    .map_err(StakeError::turn_into)?;

                let mut destination_meta = source_meta;
                destination_meta.rent_exempt_reserve =
                    validated_split_info.destination_rent_exempt_reserve;

                set_stake_state(
                    source_stake_account_info,
                    &StakeStateV2::Stake(source_meta, source_stake, stake_flags),
                )?;

                set_stake_state(
                    destination_stake_account_info,
                    &StakeStateV2::Stake(destination_meta, destination_stake, stake_flags),
                )?;
            }
            StakeStateV2::Initialized(source_meta) => {
                source_meta
                    .authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(InstructionError::turn_into)?;

                // XXX note this function also internally summons Rent via syscall
                let validated_split_info = validate_split_amount(
                    source_lamport_balance,
                    destination_lamport_balance,
                    split_lamports,
                    &source_meta,
                    destination_data_len,
                    0,     // additional_required_lamports
                    false, // is_active
                )?;

                let mut destination_meta = source_meta;
                destination_meta.rent_exempt_reserve =
                    validated_split_info.destination_rent_exempt_reserve;

                set_stake_state(
                    destination_stake_account_info,
                    &StakeStateV2::Initialized(destination_meta),
                )?;
            }
            StakeStateV2::Uninitialized => {
                if !signers.contains(source_stake_account_info.key) {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
            _ => return Err(ProgramError::InvalidAccountData),
        }

        // Deinitialize state upon zero balance
        if split_lamports == source_lamport_balance {
            set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;
        }

        // XXX are there nicer helpers for AccountInfo? checked_{add,sub}_lamports dont exist
        let mut source_lamports = source_stake_account_info.try_borrow_mut_lamports()?;
        **source_lamports = source_lamports
            .checked_sub(split_lamports)
            .ok_or(ProgramError::InsufficientFunds)?;

        let mut destination_lamports = destination_stake_account_info.try_borrow_mut_lamports()?;
        **destination_lamports = destination_lamports
            .checked_add(split_lamports)
            .ok_or(ProgramError::InsufficientFunds)?;

        Ok(())
    }

    fn process_withdraw(accounts: &[AccountInfo], withdraw_lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let stake_history_info = next_account_info(account_info_iter)?;
        let stake_history = &StakeHistoryData::from_account_info(stake_history_info)?;
        let withdraw_authority_info = next_account_info(account_info_iter)?;
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        // XXX as noted in the function itself, this is stubbed out and needs to be solved in monorepo
        let new_rate_activation_epoch = new_warmup_cooldown_rate_epoch();

        // this is somewhat subtle, but if the stake account is Uninitialized, you pass it twice and sign
        // ie, Initialized or Stake, we use real withdraw authority. Uninitialized, stake account is its own authority
        let (signers, custodian) = if let Some(lockup_authority_info) = option_lockup_authority_info
        {
            (
                collect_signers(&[withdraw_authority_info, lockup_authority_info], true)?,
                Some(lockup_authority_info.key),
            )
        } else {
            (collect_signers(&[withdraw_authority_info], true)?, None)
        };

        let (lockup, reserve, is_staked) = match get_stake_state(source_stake_account_info)? {
            StakeStateV2::Stake(meta, stake, _stake_flag) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Withdrawer)
                    .map_err(InstructionError::turn_into)?;
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
                    .check(&signers, StakeAuthorize::Withdrawer)
                    .map_err(InstructionError::turn_into)?;
                // stake accounts must have a balance >= rent_exempt_reserve
                (meta.lockup, meta.rent_exempt_reserve, false)
            }
            StakeStateV2::Uninitialized => {
                if !signers.contains(source_stake_account_info.key) {
                    return Err(ProgramError::MissingRequiredSignature);
                }
                (Lockup::default(), 0, false) // no lockup, no restrictions
            }
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // verify that lockup has expired or that the withdrawal is signed by the custodian
        // both epoch and unix_timestamp must have passed
        if lockup.is_in_force(clock, custodian) {
            return Err(StakeError::LockupInForce.turn_into());
        }

        let withdraw_lamports_and_reserve = checked_add(withdraw_lamports, reserve)?;
        let stake_account_lamports = source_stake_account_info.lamports();

        // if the stake is active, we mustn't allow the account to go away
        if is_staked && withdraw_lamports_and_reserve > stake_account_lamports {
            return Err(ProgramError::InsufficientFunds);
        }

        // a partial withdrawal must not deplete the reserve
        if withdraw_lamports != stake_account_lamports
            && withdraw_lamports_and_reserve > stake_account_lamports
        {
            // XXX why is this assert here...
            assert!(!is_staked);
            return Err(ProgramError::InsufficientFunds);
        }

        // Deinitialize state upon zero balance
        if withdraw_lamports == stake_account_lamports {
            set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;
        }

        // XXX are there nicer helpers for AccountInfo? checked_{add,sub}_lamports dont exist
        let mut source_lamports = source_stake_account_info.try_borrow_mut_lamports()?;
        **source_lamports = source_lamports
            .checked_sub(withdraw_lamports)
            .ok_or(ProgramError::InsufficientFunds)?;

        let mut destination_lamports = destination_stake_account_info.try_borrow_mut_lamports()?;
        **destination_lamports = destination_lamports
            .checked_add(withdraw_lamports)
            .ok_or(ProgramError::InsufficientFunds)?;

        Ok(())
    }

    /// Processes [Instruction](enum.Instruction.html).
    // XXX the existing program returns InstructionError not ProgramError
    // look into if theres a trait i can impl to not break the interface but modrenize
    pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
        // convenience so we can safely use id() everywhere
        if *program_id != id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        // XXX limited_deserialize?
        let instruction =
            bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)?;

        // TODO
        // * split: complictated but no blockers
        // * withdraw: complicated, requires stake history
        // * getminimumdelegation: probably trivial
        // * deactivatedelinquent: simple but requires deactivate
        // * redelegate: simple, requires stake history
        //   update we are officially NOT porting redelegate
        // plus a handful of checked and seed variants
        //
        // stake history doesnt seem too bad honestly
        // i believe we only need get(), not add(). unclear if we need to be able to iterate
        // for get() we need to binary search the vec for a given epoch entry
        // each vec item is four u64: epoch and entry, which is (effective stake, activating stake, deactivating stake)
        // this is fairly straightforward to implement with incremental parsing:
        // * decode the vec length
        // * jump to some offset
        // * decode four u64, check values
        // * repeat from 2 or return result
        // we can also be sure the data is well-formed because we check the hardcoded account key
        // how to use this with existing functions is somewhat trickier
        // we could create a GetStakeHistoryEntry typeclass and change the function interfaces
        // and make a new struct StakeHistoryAccountData or something which impls it
        // we just want one function get_entry() which does the same thing as get()
        // and then deprecate get(). itll be fun to write probably
        match instruction {
            StakeInstruction::Initialize(authorize, lockup) => {
                msg!("Instruction: Initialize");
                Self::process_initialize(accounts, authorize, lockup)
            }
            StakeInstruction::Authorize(new_authority, authority_type) => {
                msg!("Instruction: Authorize");
                Self::process_authorize(accounts, new_authority, authority_type)
            }
            StakeInstruction::DelegateStake => {
                msg!("Instruction: DelegateStake");

                if !crate::FEATURE_REDUCE_STAKE_WARMUP_COOLDOWN {
                    panic!("we only impl the `reduce_stake_warmup_cooldown` logic");
                }

                Self::process_delegate(accounts)
            }
            StakeInstruction::Deactivate => {
                msg!("Instruction: Deactivate");

                Self::process_deactivate(accounts)
            }
            StakeInstruction::SetLockup(lockup) => {
                msg!("Instruction: SetLockup");
                Self::process_set_lockup(accounts, lockup)
            }
            StakeInstruction::Merge => {
                msg!("Instruction: Merge");

                Self::process_merge(accounts)
            }
            StakeInstruction::Split(lamports) => {
                msg!("Instruction: Split");

                Self::process_split(accounts, lamports)
            }
            StakeInstruction::Withdraw(lamports) => {
                msg!("Instruction: Withdraw");

                Self::process_withdraw(accounts, lamports)
            }
            StakeInstruction::AuthorizeChecked(authority_type) => {
                msg!("Instruction: AuthorizeChecked");
                Self::process_authorize_checked(accounts, authority_type)
            }
            StakeInstruction::SetLockupChecked(lockup_checked) => {
                msg!("Instruction: SetLockup");
                Self::process_set_lockup_checked(accounts, lockup_checked)
            }
            _ => unimplemented!(),
        }
    }
}
