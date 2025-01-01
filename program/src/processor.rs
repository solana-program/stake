use {
    crate::{helpers::*, id, PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH},
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::Clock,
        entrypoint::ProgramResult,
        msg,
        program::set_return_data,
        program_error::ProgramError,
        pubkey::Pubkey,
        rent::Rent,
        stake::{
            instruction::{
                AuthorizeCheckedWithSeedArgs, AuthorizeWithSeedArgs, LockupArgs, LockupCheckedArgs,
                StakeError, StakeInstruction,
            },
            stake_flags::StakeFlags,
            state::{Authorized, Lockup, Meta, StakeAuthorize, StakeStateV2},
            tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
        },
        sysvar::{epoch_rewards::EpochRewards, stake_history::StakeHistorySysvar, Sysvar},
        vote::{program as solana_vote_program, state::VoteState},
    },
    std::{collections::HashSet, mem::MaybeUninit},
};

fn get_vote_state(vote_account_info: &AccountInfo) -> Result<Box<VoteState>, ProgramError> {
    if *vote_account_info.owner != solana_vote_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let mut vote_state = Box::new(MaybeUninit::uninit());
    VoteState::deserialize_into_uninit(&vote_account_info.try_borrow_data()?, vote_state.as_mut())
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let vote_state = unsafe { Box::from_raw(Box::into_raw(vote_state) as *mut VoteState) };

    Ok(vote_state)
}

fn get_stake_state(stake_account_info: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if *stake_account_info.owner != id() {
        return Err(ProgramError::InvalidAccountOwner);
    }

    stake_account_info
        .deserialize_data()
        .map_err(|_| ProgramError::InvalidAccountData)
}

fn set_stake_state(stake_account_info: &AccountInfo, new_state: &StakeStateV2) -> ProgramResult {
    let serialized_size =
        bincode::serialized_size(new_state).map_err(|_| ProgramError::InvalidAccountData)?;
    if serialized_size > stake_account_info.data_len() as u64 {
        return Err(ProgramError::AccountDataTooSmall);
    }

    bincode::serialize_into(&mut stake_account_info.data.borrow_mut()[..], new_state)
        .map_err(|_| ProgramError::InvalidAccountData)
}

// dont call this "move" because we have an instruction MoveLamports
fn relocate_lamports(
    source_account_info: &AccountInfo,
    destination_account_info: &AccountInfo,
    lamports: u64,
) -> ProgramResult {
    {
        let mut source_lamports = source_account_info.try_borrow_mut_lamports()?;
        **source_lamports = source_lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::InsufficientFunds)?;
    }

    {
        let mut destination_lamports = destination_account_info.try_borrow_mut_lamports()?;
        **destination_lamports = destination_lamports
            .checked_add(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }

    Ok(())
}

// almost all native stake program processors accumulate every account signer
// they then defer all signer validation to functions on Meta or Authorized
// this results in an instruction interface that is much looser than the one documented
// to avoid breaking backwards compatibility, we do the same here
// in the future, we may decide to tighten the interface and break badly formed transactions
fn collect_signers(accounts: &[AccountInfo]) -> HashSet<Pubkey> {
    let mut signers = HashSet::new();

    for account in accounts {
        if account.is_signer {
            signers.insert(*account.key);
        }
    }

    signers
}

// MoveStake, MoveLamports, Withdraw, and AuthorizeWithSeed assemble signers explicitly
fn collect_signers_checked<'a>(
    authority_info: Option<&'a AccountInfo>,
    custodian_info: Option<&'a AccountInfo>,
) -> Result<(HashSet<Pubkey>, Option<&'a Pubkey>), ProgramError> {
    let mut signers = HashSet::new();

    if let Some(authority_info) = authority_info {
        if authority_info.is_signer {
            signers.insert(*authority_info.key);
        } else {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    let custodian = if let Some(custodian_info) = custodian_info {
        if custodian_info.is_signer {
            signers.insert(*custodian_info.key);
            Some(custodian_info.key)
        } else {
            return Err(ProgramError::MissingRequiredSignature);
        }
    } else {
        None
    };

    Ok((signers, custodian))
}

fn do_initialize(
    stake_account_info: &AccountInfo,
    authorized: Authorized,
    lockup: Lockup,
    rent: &Rent,
) -> ProgramResult {
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

            set_stake_state(stake_account_info, &stake_state)
        } else {
            Err(ProgramError::InsufficientFunds)
        }
    } else {
        Err(ProgramError::InvalidAccountData)
    }
}

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
                    signers,
                    new_authority,
                    authority_type,
                    Some((&meta.lockup, clock, custodian)),
                )
                .map_err(to_program_error)?;

            set_stake_state(stake_account_info, &StakeStateV2::Initialized(meta))
        }
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.authorized
                .authorize(
                    signers,
                    new_authority,
                    authority_type,
                    Some((&meta.lockup, clock, custodian)),
                )
                .map_err(to_program_error)?;

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
    signers: &HashSet<Pubkey>,
    lockup: &LockupArgs,
    clock: &Clock,
) -> ProgramResult {
    match get_stake_state(stake_account_info)? {
        StakeStateV2::Initialized(mut meta) => {
            meta.set_lockup(lockup, signers, clock)
                .map_err(to_program_error)?;

            set_stake_state(stake_account_info, &StakeStateV2::Initialized(meta))
        }
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.set_lockup(lockup, signers, clock)
                .map_err(to_program_error)?;

            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, stake_flags),
            )
        }
        _ => Err(ProgramError::InvalidAccountData),
    }
}

fn move_stake_or_lamports_shared_checks(
    source_stake_account_info: &AccountInfo,
    lamports: u64,
    destination_stake_account_info: &AccountInfo,
    stake_authority_info: &AccountInfo,
) -> Result<(MergeKind, MergeKind), ProgramError> {
    // authority must sign
    let (signers, _) = collect_signers_checked(Some(stake_authority_info), None)?;

    // confirm not the same account
    if *source_stake_account_info.key == *destination_stake_account_info.key {
        return Err(ProgramError::InvalidInstructionData);
    }

    // source and destination must be writable
    // runtime guards against unowned writes, but MoveStake and MoveLamports are defined by SIMD
    // we check explicitly to avoid any possibility of a successful no-op that never attempts to write
    if !source_stake_account_info.is_writable || !destination_stake_account_info.is_writable {
        return Err(ProgramError::InvalidInstructionData);
    }

    // must move something
    if lamports == 0 {
        return Err(ProgramError::InvalidArgument);
    }

    let clock = Clock::get()?;
    let stake_history = StakeHistorySysvar(clock.epoch);

    // get_if_mergeable ensures accounts are not partly activated or in any form of deactivating
    // we still need to exclude activating state ourselves
    let source_merge_kind = MergeKind::get_if_mergeable(
        &get_stake_state(source_stake_account_info)?,
        source_stake_account_info.lamports(),
        &clock,
        &stake_history,
    )?;

    // Authorized staker is allowed to move stake
    source_merge_kind
        .meta()
        .authorized
        .check(&signers, StakeAuthorize::Staker)
        .map_err(to_program_error)?;

    // same transient assurance as with source
    let destination_merge_kind = MergeKind::get_if_mergeable(
        &get_stake_state(destination_stake_account_info)?,
        destination_stake_account_info.lamports(),
        &clock,
        &stake_history,
    )?;

    // ensure all authorities match and lockups match if lockup is in force
    MergeKind::metas_can_merge(
        source_merge_kind.meta(),
        destination_merge_kind.meta(),
        &clock,
    )?;

    Ok((source_merge_kind, destination_merge_kind))
}

// NOTE our usage of the accounts iter is idiosyncratic, in imitation of the native stake program
// native stake typically accumulates all signers from the accounts array indiscriminately
// each instruction processor also asserts a required number of instruction accounts
// but this is extremely ad hoc, essentially allowing any account to act as a signing authority
// when lengths are asserted in setup, accounts are retrieved via hardcoded index from InstructionContext
// but after control is passed to main processing functions, they are pulled from the TransactionContext
//
// we aim to implement this behavior exactly, such that both programs are consensus compatible:
// * all transactions that would fail on one program also fail on the other
// * all transactions that would succeed on one program also succeed on the other
// * for successful transactions, all account state transitions are identical
// error codes and log output may differ
//
// this is not strictly necessary, since the switchover will be feature-gated. so this is not a security issue
// mostly its so no one can blame the bpf switchover for breaking their usecase, even pathological ones
//
// in service to this end, all accounts iters are commented with how the native program uses them
// for accounts that should always, or almost always, exist, which the native program does not assert...
// ...we leave a commented-out `next_account_info()` call, to aid in a future refactor
// after the bpf switchover ships, we may update to strictly assert these accounts exist
pub struct Processor {}
impl Processor {
    fn process_initialize(
        accounts: &[AccountInfo],
        authorized: Authorized,
        lockup: Lockup,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 2 accounts (1 sysvar)
        let stake_account_info = next_account_info(account_info_iter)?;
        let rent_info = next_account_info(account_info_iter)?;

        let rent = &Rent::from_account_info(rent_info)?;

        // `get_stake_state()` is called unconditionally, which checks owner
        do_initialize(stake_account_info, authorized, lockup, rent)?;

        Ok(())
    }

    fn process_authorize(
        accounts: &[AccountInfo],
        new_authority: Pubkey,
        authority_type: StakeAuthorize,
    ) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 3 accounts (1 sysvar)
        let stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let _stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;

        // other accounts
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let clock = &Clock::from_account_info(clock_info)?;

        let custodian = option_lockup_authority_info
            .filter(|a| a.is_signer)
            .map(|a| a.key);

        // `get_stake_state()` is called unconditionally, which checks owner
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

    fn process_delegate(accounts: &[AccountInfo]) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 5 accounts (2 sysvars + stake config)
        let stake_account_info = next_account_info(account_info_iter)?;
        let vote_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let _stake_history_info = next_account_info(account_info_iter)?;
        let _stake_config_info = next_account_info(account_info_iter)?;

        // other accounts
        // let _stake_authority_info = next_account_info(account_info_iter);

        let clock = &Clock::from_account_info(clock_info)?;
        let stake_history = &StakeHistorySysvar(clock.epoch);

        let vote_state = get_vote_state(vote_account_info)?;

        match get_stake_state(stake_account_info)? {
            StakeStateV2::Initialized(meta) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(to_program_error)?;

                let ValidatedDelegatedInfo { stake_amount } =
                    validate_delegated_amount(stake_account_info, &meta)?;

                let stake = new_stake(
                    stake_amount,
                    vote_account_info.key,
                    &vote_state,
                    clock.epoch,
                );

                set_stake_state(
                    stake_account_info,
                    &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
                )
            }
            StakeStateV2::Stake(meta, mut stake, flags) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(to_program_error)?;

                let ValidatedDelegatedInfo { stake_amount } =
                    validate_delegated_amount(stake_account_info, &meta)?;

                redelegate_stake(
                    &mut stake,
                    stake_amount,
                    vote_account_info.key,
                    &vote_state,
                    clock.epoch,
                    stake_history,
                )?;

                set_stake_state(stake_account_info, &StakeStateV2::Stake(meta, stake, flags))
            }
            _ => Err(ProgramError::InvalidAccountData),
        }?;

        Ok(())
    }

    fn process_split(accounts: &[AccountInfo], split_lamports: u64) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 2 accounts
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_stake_account_info = next_account_info(account_info_iter)?;

        // other accounts
        // let _stake_authority_info = next_account_info(account_info_iter);

        let clock = Clock::get()?;
        let stake_history = &StakeHistorySysvar(clock.epoch);

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
                    .map_err(to_program_error)?;

                let minimum_delegation = crate::get_minimum_delegation();

                let status = source_stake.delegation.stake_activating_and_deactivating(
                    clock.epoch,
                    stake_history,
                    PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
                );

                let is_active = status.effective > 0;

                // NOTE this function also internally summons Rent via syscall
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
                        // requested, less any lamports needed to cover the
                        // split_rent_exempt_reserve.
                        if source_stake.delegation.stake.saturating_sub(split_lamports)
                            < minimum_delegation
                        {
                            return Err(StakeError::InsufficientDelegation.into());
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
                    return Err(StakeError::InsufficientDelegation.into());
                }

                let destination_stake =
                    source_stake.split(remaining_stake_delta, split_stake_amount)?;

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
                    .map_err(to_program_error)?;

                // NOTE this function also internally summons Rent via syscall
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
                if !source_stake_account_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
            _ => return Err(ProgramError::InvalidAccountData),
        }

        // Deinitialize state upon zero balance
        if split_lamports == source_lamport_balance {
            set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;
        }

        relocate_lamports(
            source_stake_account_info,
            destination_stake_account_info,
            split_lamports,
        )?;

        Ok(())
    }

    fn process_withdraw(accounts: &[AccountInfo], withdraw_lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 5 accounts (2 sysvars)
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let _stake_history_info = next_account_info(account_info_iter)?;
        let withdraw_authority_info = next_account_info(account_info_iter)?;

        // other accounts
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let clock = &Clock::from_account_info(clock_info)?;
        let stake_history = &StakeHistorySysvar(clock.epoch);

        // this is somewhat subtle. for Initialized and Stake, there is a real authority
        // but for Uninitialized, the source account is passed twice, and signed for
        let (signers, custodian) =
            collect_signers_checked(Some(withdraw_authority_info), option_lockup_authority_info)?;

        let (lockup, reserve, is_staked) = match get_stake_state(source_stake_account_info)? {
            StakeStateV2::Stake(meta, stake, _stake_flag) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Withdrawer)
                    .map_err(to_program_error)?;
                // if we have a deactivation epoch and we're in cooldown
                let staked = if clock.epoch >= stake.delegation.deactivation_epoch {
                    stake.delegation.stake(
                        clock.epoch,
                        stake_history,
                        PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
                    )
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
                    .map_err(to_program_error)?;
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

        // verify that lockup has expired or that the withdrawal is signed by the
        // custodian both epoch and unix_timestamp must have passed
        if lockup.is_in_force(clock, custodian) {
            return Err(StakeError::LockupInForce.into());
        }

        let stake_account_lamports = source_stake_account_info.lamports();
        if withdraw_lamports == stake_account_lamports {
            // if the stake is active, we mustn't allow the account to go away
            if is_staked {
                return Err(ProgramError::InsufficientFunds);
            }

            // Deinitialize state upon zero balance
            set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;
        } else {
            // a partial withdrawal must not deplete the reserve
            let withdraw_lamports_and_reserve = checked_add(withdraw_lamports, reserve)?;
            if withdraw_lamports_and_reserve > stake_account_lamports {
                return Err(ProgramError::InsufficientFunds);
            }
        }

        relocate_lamports(
            source_stake_account_info,
            destination_info,
            withdraw_lamports,
        )?;

        Ok(())
    }

    fn process_deactivate(accounts: &[AccountInfo]) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 2 accounts (1 sysvar)
        let stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;

        // other accounts
        // let _stake_authority_info = next_account_info(account_info_iter);

        let clock = &Clock::from_account_info(clock_info)?;

        match get_stake_state(stake_account_info)? {
            StakeStateV2::Stake(meta, mut stake, stake_flags) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(to_program_error)?;

                stake.deactivate(clock.epoch)?;

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
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 1 account
        let stake_account_info = next_account_info(account_info_iter)?;

        // other accounts
        // let _old_withdraw_or_lockup_authority_info = next_account_info(account_info_iter);

        let clock = Clock::get()?;

        // `get_stake_state()` is called unconditionally, which checks owner
        do_set_lockup(stake_account_info, &signers, &lockup, &clock)?;

        Ok(())
    }

    fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 4 accounts (2 sysvars)
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let _stake_history_info = next_account_info(account_info_iter)?;

        // other accounts
        // let _stake_authority_info = next_account_info(account_info_iter);

        let clock = &Clock::from_account_info(clock_info)?;
        let stake_history = &StakeHistorySysvar(clock.epoch);

        if source_stake_account_info.key == destination_stake_account_info.key {
            return Err(ProgramError::InvalidArgument);
        }

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
        relocate_lamports(
            source_stake_account_info,
            destination_stake_account_info,
            source_stake_account_info.lamports(),
        )?;

        Ok(())
    }

    fn process_authorize_with_seed(
        accounts: &[AccountInfo],
        authorize_args: AuthorizeWithSeedArgs,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 3 accounts (1 sysvar)
        let stake_account_info = next_account_info(account_info_iter)?;
        let stake_or_withdraw_authority_base_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;

        // other accounts
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let clock = &Clock::from_account_info(clock_info)?;

        let (mut signers, custodian) = collect_signers_checked(None, option_lockup_authority_info)?;

        if stake_or_withdraw_authority_base_info.is_signer {
            signers.insert(Pubkey::create_with_seed(
                stake_or_withdraw_authority_base_info.key,
                &authorize_args.authority_seed,
                &authorize_args.authority_owner,
            )?);
        }

        // `get_stake_state()` is called unconditionally, which checks owner
        do_authorize(
            stake_account_info,
            &signers,
            &authorize_args.new_authorized_pubkey,
            authorize_args.stake_authorize,
            custodian,
            clock,
        )?;

        Ok(())
    }

    fn process_initialize_checked(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 4 accounts (1 sysvar)
        let stake_account_info = next_account_info(account_info_iter)?;
        let rent_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;
        let withdraw_authority_info = next_account_info(account_info_iter)?;

        let rent = &Rent::from_account_info(rent_info)?;

        if !withdraw_authority_info.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let authorized = Authorized {
            staker: *stake_authority_info.key,
            withdrawer: *withdraw_authority_info.key,
        };

        // `get_stake_state()` is called unconditionally, which checks owner
        do_initialize(stake_account_info, authorized, Lockup::default(), rent)?;

        Ok(())
    }

    fn process_authorize_checked(
        accounts: &[AccountInfo],
        authority_type: StakeAuthorize,
    ) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 4 accounts (1 sysvar)
        let stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let _old_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
        let new_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;

        // other accounts
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let clock = &Clock::from_account_info(clock_info)?;

        if !new_stake_or_withdraw_authority_info.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let custodian = option_lockup_authority_info
            .filter(|a| a.is_signer)
            .map(|a| a.key);

        // `get_stake_state()` is called unconditionally, which checks owner
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

    fn process_authorize_checked_with_seed(
        accounts: &[AccountInfo],
        authorize_args: AuthorizeCheckedWithSeedArgs,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 4 accounts (1 sysvar)
        let stake_account_info = next_account_info(account_info_iter)?;
        let old_stake_or_withdraw_authority_base_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let new_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;

        // other accounts
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let clock = &Clock::from_account_info(clock_info)?;

        let (mut signers, custodian) = collect_signers_checked(
            Some(new_stake_or_withdraw_authority_info),
            option_lockup_authority_info,
        )?;

        if old_stake_or_withdraw_authority_base_info.is_signer {
            signers.insert(Pubkey::create_with_seed(
                old_stake_or_withdraw_authority_base_info.key,
                &authorize_args.authority_seed,
                &authorize_args.authority_owner,
            )?);
        }

        // `get_stake_state()` is called unconditionally, which checks owner
        do_authorize(
            stake_account_info,
            &signers,
            new_stake_or_withdraw_authority_info.key,
            authorize_args.stake_authorize,
            custodian,
            clock,
        )?;

        Ok(())
    }

    fn process_set_lockup_checked(
        accounts: &[AccountInfo],
        lockup_checked: LockupCheckedArgs,
    ) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // native asserts: 1 account
        let stake_account_info = next_account_info(account_info_iter)?;

        // other accounts
        let _old_withdraw_or_lockup_authority_info = next_account_info(account_info_iter);
        let option_new_lockup_authority_info = next_account_info(account_info_iter).ok();

        let clock = Clock::get()?;

        let custodian = match option_new_lockup_authority_info {
            Some(new_lockup_authority_info) if new_lockup_authority_info.is_signer => {
                Some(new_lockup_authority_info.key)
            }
            Some(_) => return Err(ProgramError::MissingRequiredSignature),
            None => None,
        };

        let lockup = LockupArgs {
            unix_timestamp: lockup_checked.unix_timestamp,
            epoch: lockup_checked.epoch,
            custodian: custodian.copied(),
        };

        // `get_stake_state()` is called unconditionally, which checks owner
        do_set_lockup(stake_account_info, &signers, &lockup, &clock)?;

        Ok(())
    }

    fn process_deactivate_delinquent(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 3 accounts
        let stake_account_info = next_account_info(account_info_iter)?;
        let delinquent_vote_account_info = next_account_info(account_info_iter)?;
        let reference_vote_account_info = next_account_info(account_info_iter)?;

        let clock = Clock::get()?;

        let delinquent_vote_state = get_vote_state(delinquent_vote_account_info)?;
        let reference_vote_state = get_vote_state(reference_vote_account_info)?;

        if !acceptable_reference_epoch_credits(&reference_vote_state.epoch_credits, clock.epoch) {
            return Err(StakeError::InsufficientReferenceVotes.into());
        }

        if let StakeStateV2::Stake(meta, mut stake, stake_flags) =
            get_stake_state(stake_account_info)?
        {
            if stake.delegation.voter_pubkey != *delinquent_vote_account_info.key {
                return Err(StakeError::VoteAddressMismatch.into());
            }

            // Deactivate the stake account if its delegated vote account has never voted or
            // has not voted in the last
            // `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`
            if eligible_for_deactivate_delinquent(&delinquent_vote_state.epoch_credits, clock.epoch)
            {
                stake.deactivate(clock.epoch)?;

                set_stake_state(
                    stake_account_info,
                    &StakeStateV2::Stake(meta, stake, stake_flags),
                )
            } else {
                Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into())
            }
        } else {
            Err(ProgramError::InvalidAccountData)
        }?;

        Ok(())
    }

    fn process_move_stake(accounts: &[AccountInfo], lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 3 accounts
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        let (source_merge_kind, destination_merge_kind) = move_stake_or_lamports_shared_checks(
            source_stake_account_info,
            lamports,
            destination_stake_account_info,
            stake_authority_info,
        )?;

        // ensure source and destination are the right size for the current version of
        // StakeState this a safeguard in case there is a new version of the
        // struct that cannot fit into an old account
        if source_stake_account_info.data_len() != StakeStateV2::size_of()
            || destination_stake_account_info.data_len() != StakeStateV2::size_of()
        {
            return Err(ProgramError::InvalidAccountData);
        }

        // source must be fully active
        let MergeKind::FullyActive(source_meta, mut source_stake) = source_merge_kind else {
            return Err(ProgramError::InvalidAccountData);
        };

        let minimum_delegation = crate::get_minimum_delegation();
        let source_effective_stake = source_stake.delegation.stake;

        // source cannot move more stake than it has, regardless of how many lamports it
        // has
        let source_final_stake = source_effective_stake
            .checked_sub(lamports)
            .ok_or(ProgramError::InvalidArgument)?;

        // unless all stake is being moved, source must retain at least the minimum
        // delegation
        if source_final_stake != 0 && source_final_stake < minimum_delegation {
            return Err(ProgramError::InvalidArgument);
        }

        // destination must be fully active or fully inactive
        let destination_meta = match destination_merge_kind {
            MergeKind::FullyActive(destination_meta, mut destination_stake) => {
                // if active, destination must be delegated to the same vote account as source
                if source_stake.delegation.voter_pubkey != destination_stake.delegation.voter_pubkey
                {
                    return Err(StakeError::VoteAddressMismatch.into());
                }

                let destination_effective_stake = destination_stake.delegation.stake;
                let destination_final_stake = destination_effective_stake
                    .checked_add(lamports)
                    .ok_or(ProgramError::ArithmeticOverflow)?;

                // ensure destination meets miniumum delegation
                // since it is already active, this only really applies if the minimum is raised
                if destination_final_stake < minimum_delegation {
                    return Err(ProgramError::InvalidArgument);
                }

                merge_delegation_stake_and_credits_observed(
                    &mut destination_stake,
                    lamports,
                    source_stake.credits_observed,
                )?;

                // StakeFlags::empty() is valid here because the only existing stake flag,
                // MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED, does not apply to
                // active stakes
                set_stake_state(
                    destination_stake_account_info,
                    &StakeStateV2::Stake(destination_meta, destination_stake, StakeFlags::empty()),
                )?;

                destination_meta
            }
            MergeKind::Inactive(destination_meta, _, _) => {
                // if destination is inactive, it must be given at least the minimum delegation
                if lamports < minimum_delegation {
                    return Err(ProgramError::InvalidArgument);
                }

                let mut destination_stake = source_stake;
                destination_stake.delegation.stake = lamports;

                // StakeFlags::empty() is valid here because the only existing stake flag,
                // MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED, is cleared when a stake
                // is activated
                set_stake_state(
                    destination_stake_account_info,
                    &StakeStateV2::Stake(destination_meta, destination_stake, StakeFlags::empty()),
                )?;

                destination_meta
            }
            _ => return Err(ProgramError::InvalidAccountData),
        };

        if source_final_stake == 0 {
            set_stake_state(
                source_stake_account_info,
                &StakeStateV2::Initialized(source_meta),
            )?;
        } else {
            source_stake.delegation.stake = source_final_stake;

            // StakeFlags::empty() is valid here because the only existing stake flag,
            // MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED, does not apply to
            // active stakes
            set_stake_state(
                source_stake_account_info,
                &StakeStateV2::Stake(source_meta, source_stake, StakeFlags::empty()),
            )?;
        }

        relocate_lamports(
            source_stake_account_info,
            destination_stake_account_info,
            lamports,
        )?;

        // this should be impossible, but because we do all our math with delegations,
        // best to guard it
        if source_stake_account_info.lamports() < source_meta.rent_exempt_reserve
            || destination_stake_account_info.lamports() < destination_meta.rent_exempt_reserve
        {
            msg!("Delegation calculations violated lamport balance assumptions");
            return Err(ProgramError::InvalidArgument);
        }

        Ok(())
    }

    fn process_move_lamports(accounts: &[AccountInfo], lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // native asserts: 3 accounts
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        let (source_merge_kind, _) = move_stake_or_lamports_shared_checks(
            source_stake_account_info,
            lamports,
            destination_stake_account_info,
            stake_authority_info,
        )?;

        let source_free_lamports = match source_merge_kind {
            MergeKind::FullyActive(source_meta, source_stake) => source_stake_account_info
                .lamports()
                .saturating_sub(source_stake.delegation.stake)
                .saturating_sub(source_meta.rent_exempt_reserve),
            MergeKind::Inactive(source_meta, source_lamports, _) => {
                source_lamports.saturating_sub(source_meta.rent_exempt_reserve)
            }
            _ => return Err(ProgramError::InvalidAccountData),
        };

        if lamports > source_free_lamports {
            return Err(ProgramError::InvalidArgument);
        }

        relocate_lamports(
            source_stake_account_info,
            destination_stake_account_info,
            lamports,
        )?;

        Ok(())
    }

    /// Processes [Instruction](enum.Instruction.html).
    pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
        // convenience so we can safely use id() everywhere
        if *program_id != id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        let epoch_rewards_active = EpochRewards::get()
            .map(|epoch_rewards| epoch_rewards.active)
            .unwrap_or(false);

        let instruction =
            bincode::deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)?;

        if epoch_rewards_active && !matches!(instruction, StakeInstruction::GetMinimumDelegation) {
            return Err(StakeError::EpochRewardsActive.into());
        }

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
                Self::process_delegate(accounts)
            }
            StakeInstruction::Split(lamports) => {
                msg!("Instruction: Split");
                Self::process_split(accounts, lamports)
            }
            StakeInstruction::Withdraw(lamports) => {
                msg!("Instruction: Withdraw");
                Self::process_withdraw(accounts, lamports)
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
            StakeInstruction::AuthorizeWithSeed(args) => {
                msg!("Instruction: AuthorizeWithSeed");
                Self::process_authorize_with_seed(accounts, args)
            }
            StakeInstruction::InitializeChecked => {
                msg!("Instruction: InitializeChecked");
                Self::process_initialize_checked(accounts)
            }
            StakeInstruction::AuthorizeChecked(authority_type) => {
                msg!("Instruction: AuthorizeChecked");
                Self::process_authorize_checked(accounts, authority_type)
            }
            StakeInstruction::AuthorizeCheckedWithSeed(args) => {
                msg!("Instruction: AuthorizeCheckedWithSeed");
                Self::process_authorize_checked_with_seed(accounts, args)
            }
            StakeInstruction::SetLockupChecked(lockup_checked) => {
                msg!("Instruction: SetLockupChecked");
                Self::process_set_lockup_checked(accounts, lockup_checked)
            }
            StakeInstruction::GetMinimumDelegation => {
                msg!("Instruction: GetMinimumDelegation");
                let minimum_delegation = crate::get_minimum_delegation();
                set_return_data(&minimum_delegation.to_le_bytes());
                Ok(())
            }
            StakeInstruction::DeactivateDelinquent => {
                msg!("Instruction: DeactivateDelinquent");
                Self::process_deactivate_delinquent(accounts)
            }
            #[allow(deprecated)]
            StakeInstruction::Redelegate => Err(ProgramError::InvalidInstructionData),
            // NOTE we assume the program is going live after `move_stake_and_move_lamports_ixs` is
            // activated
            StakeInstruction::MoveStake(lamports) => {
                msg!("Instruction: MoveStake");
                Self::process_move_stake(accounts, lamports)
            }
            StakeInstruction::MoveLamports(lamports) => {
                msg!("Instruction: MoveLamports");
                Self::process_move_lamports(accounts, lamports)
            }
        }
    }
}
