#![allow(dead_code)]
#![allow(unused_imports)]

use {
    crate::{feature_set_die, id, stake_history_die},
    num_traits::cast::ToPrimitive,
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::{Clock, Epoch},
        entrypoint::ProgramResult,
        instruction::{checked_add, InstructionError},
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

// HANA as noted... elsewhere... in a monorepo fork...
// this is used by Deactivate and DeactivateDelinquent
// it was added as part of the stake flags pr, to deal with potential abuse of redelegate to force early deactivation
// unfortunately these instructions (and Redelegate) should require the stake history sysvar but dont
// i should pr monorepo to get something in under the redelegate feature but i think the move with that is:
// * redelegate requires stake history (nonbreaking, currently nonactive instruction)
// * deactivate optionally requires stake history (only for accounts that have been redelegated)
// * deactivate delinquent optionally requires stake history (only for accounts that have been redelegated)
//   alternatively we allow deactivate deqlinquent to yolo it but probably the first approach is better
//   it would technically break backwards compat but i cant imagine there are any workflows that depend on this
fn do_deactivate_stake(
    stake: &mut Stake,
    stake_flags: &mut StakeFlags,
    epoch: Epoch,
    option_stake_history: Option<StakeHistoryData>,
) -> ProgramResult {
    if crate::FEATURE_STAKE_REDELEGATE_INSTRUCTION {
        if stake_flags.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED) {
            let stake_history = match option_stake_history {
                Some(stake_history) => stake_history,
                None => return Err(ProgramError::NotEnoughAccountKeys),
            };

            // when MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED flag is set on stake_flags,
            // deactivation is only permitted when the stake delegation activating amount is zero.
            let status = stake.delegation.stake_activating_and_deactivating(
                epoch,
                &stake_history,
                new_warmup_cooldown_rate_epoch(),
            );

            if status.activating != 0 {
                Err(
                    StakeError::RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted
                        .turn_into(),
                )
            } else {
                stake.deactivate(epoch).map_err(StakeError::turn_into)?;
                // After deactivation, need to clear `MustFullyActivateBeforeDeactivationIsPermitted` flag if any.
                // So that future activation and deactivation are not subject to that restriction.
                stake_flags
                    .remove(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
                Ok(())
            }
        } else {
            stake.deactivate(epoch).map_err(StakeError::turn_into)?;
            Ok(())
        }
    } else {
        stake.deactivate(epoch).map_err(StakeError::turn_into)?;
        Ok(())
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
            StakeStateV2::Stake(meta, mut _stake, _stake_flags) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(InstructionError::turn_into)?;

                let ValidatedDelegatedInfo { stake_amount: _ } =
                    validate_delegated_amount(&stake_account_info, &meta)?;

                // TODO redelegate, then set state
                unimplemented!()
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
        let option_stake_history_info = next_account_info(account_info_iter).ok();

        let signers = collect_signers(&[stake_authority_info], false)?;

        let option_stake_history = option_stake_history_info
            .and_then(|info| StakeHistoryData::from_account_info(info).ok());

        match get_stake_state(stake_account_info)? {
            StakeStateV2::Stake(meta, mut stake, mut stake_flags) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(InstructionError::turn_into)?;

                do_deactivate_stake(
                    &mut stake,
                    &mut stake_flags,
                    clock.epoch,
                    option_stake_history,
                )?;

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
        let stake_authority_info = next_account_info(account_info_iter)?;

        if source_stake_account_info.key == destination_stake_account_info.key {
            return Err(ProgramError::InvalidArgument);
        }

        let mut signers = HashSet::new();

        if stake_authority_info.is_signer {
            signers.insert(*stake_authority_info.key);
        }

        let signers = signers;

        Ok(())
    }

    /// Processes [Instruction](enum.Instruction.html).
    // XXX the existing program returns InstructionError not ProgramError
    // look into if theres a trait i can impl to not break the interface but modrenize
    pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
        if *program_id != id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        // XXX limited_deserialize?
        let instruction =
            bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)?;

        // TODO
        // * split: complictated but no blockers
        // * withdraw: complicated, requires stake history
        // * deactivate: fairly simple, requires stake history
        // * merge: simple in program but i will need to mess with MergeKind. requires stake history
        //   update lol mergekind is ours, not program or sdk. easy
        // * getminimumdelegation: probably trivial
        // * deactivatedelinquent: simple but requires deactivate
        // * redelegate: simple, requires stake history
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
