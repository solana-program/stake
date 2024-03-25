use {
    crate::{helpers::*, id, PERPETUAL_NEW_WARMUP},
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::Clock,
        entrypoint::ProgramResult,
        instruction::InstructionError,
        msg,
        program::set_return_data,
        program_error::ProgramError,
        pubkey::Pubkey,
        rent::Rent,
        stake::state::{Meta, StakeAuthorize, StakeStateV2},
        stake::{
            instruction::{
                AuthorizeCheckedWithSeedArgs, AuthorizeWithSeedArgs, LockupArgs, LockupCheckedArgs,
                StakeError, StakeInstruction,
            },
            stake_flags::StakeFlags,
            state::{Authorized, Lockup},
            tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
        },
        stake_history::StakeHistorySyscall,
        sysvar::Sysvar,
        vote::program as solana_vote_program,
        vote::state::VoteState,
    },
    std::collections::HashSet,
};

// XXX a nice change would be to pop an account off the queue and discard if its a gettable sysvar
// ie, allow people to omit them from the accounts list without breaking compat

fn get_vote_state(vote_account_info: &AccountInfo) -> Result<Box<VoteState>, ProgramError> {
    if *vote_account_info.owner != solana_vote_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let mut vote_state = Box::<VoteState>::default();
    VoteState::deserialize_into(&vote_account_info.data.borrow(), &mut vote_state)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    Ok(vote_state)
}

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

// various monorepo functions expect a HashSet of signer pubkeys. this constructs it
// the unchecked mode doesnt add pubkeys of non-signers, relying on downstream errors if a required signer is missing
// the checked mode expects every AccountInfo passed in should be a signer and errors if any is not
fn collect_signers<'a>(
    accounts: &[&'a AccountInfo],
    optional_account: Option<&'a AccountInfo>,
    checked: bool,
) -> Result<(HashSet<Pubkey>, Option<&'a Pubkey>), ProgramError> {
    let mut signers = HashSet::new();

    for account in accounts {
        if account.is_signer {
            signers.insert(*account.key);
        } else if checked {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    let custodian = if let Some(account) = optional_account {
        if account.is_signer {
            signers.insert(*account.key);
            Some(account.key)
        } else if checked {
            return Err(ProgramError::MissingRequiredSignature);
        } else {
            None
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
                .map_err(InstructionError::turn_into)?;

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

        do_initialize(stake_account_info, authorized, lockup, rent)?;

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

        let (signers, custodian) = collect_signers(
            &[stake_or_withdraw_authority_info],
            option_lockup_authority_info,
            false,
        )?;

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
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let vote_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let _stake_history_info = next_account_info(account_info_iter)?;
        let _stake_config_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        let stake_history = &StakeHistorySyscall::default();

        let (signers, _) = collect_signers(&[stake_authority_info], None, false)?;

        let vote_state = get_vote_state(vote_account_info)?;

        match get_stake_state(stake_account_info)? {
            StakeStateV2::Initialized(meta) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(InstructionError::turn_into)?;

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
                    .map_err(InstructionError::turn_into)?;

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
            // XXX TODO FIXME this is incorrect, obviously we need to be able to delegate a deactivated account
            // but when i was adapting the code, this goes through redelegate, which we removed
            // i need to look at what the program did *before* redelegate was added and copy *that*
            _ => Err(ProgramError::InvalidAccountData),
        }?;

        Ok(())
    }

    fn process_split(accounts: &[AccountInfo], split_lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        // XXX TODO FIXME this replicates the behavior of the existing program but probably better to check
        let (signers, _) = collect_signers(&[stake_authority_info], None, false)?;

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

                let is_active = if crate::FEATURE_REQUIRE_RENT_EXEMPT_SPLIT_DESTINATION {
                    let clock = Clock::get()?;
                    let stake_history = &StakeHistorySyscall::default();

                    let status = source_stake.delegation.stake_activating_and_deactivating(
                        clock.epoch,
                        stake_history,
                        PERPETUAL_NEW_WARMUP,
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
        let _stake_history_info = next_account_info(account_info_iter)?;
        let stake_history = &StakeHistorySyscall::default();
        let withdraw_authority_info = next_account_info(account_info_iter)?;
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        // this is somewhat subtle, but if the stake account is Uninitialized, you pass it twice and sign
        // ie, Initialized or Stake, we use real withdraw authority. Uninitialized, stake account is its own authority
        let (signers, custodian) = collect_signers(
            &[withdraw_authority_info],
            option_lockup_authority_info,
            true,
        )?;

        let (lockup, reserve, is_staked) = match get_stake_state(source_stake_account_info)? {
            StakeStateV2::Stake(meta, stake, _stake_flag) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Withdrawer)
                    .map_err(InstructionError::turn_into)?;
                // if we have a deactivation epoch and we're in cooldown
                let staked = if clock.epoch >= stake.delegation.deactivation_epoch {
                    stake
                        .delegation
                        .stake(clock.epoch, stake_history, PERPETUAL_NEW_WARMUP)
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

    fn process_deactivate(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let stake_authority_info = next_account_info(account_info_iter)?;

        let (signers, _) = collect_signers(&[stake_authority_info], None, false)?;

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

        let (signers, _) = collect_signers(&[old_withdraw_or_lockup_authority_info], None, false)?;

        do_set_lockup(stake_account_info, signers, &lockup, &clock)?;

        Ok(())
    }

    fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let _stake_history_info = next_account_info(account_info_iter)?;
        let stake_history = &StakeHistorySyscall::default();
        let stake_authority_info = next_account_info(account_info_iter)?;

        if source_stake_account_info.key == destination_stake_account_info.key {
            return Err(ProgramError::InvalidArgument);
        }

        // XXX TODO FIXME this replicates the behavior of the existing program but probably better to check
        // do it after i can test this program lol
        let (signers, _) = collect_signers(&[stake_authority_info], None, false)?;

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

    fn process_authorize_with_seed(
        accounts: &[AccountInfo],
        authorize_args: AuthorizeWithSeedArgs,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let stake_or_withdraw_authority_base_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let (mut signers, custodian) = collect_signers(&[], option_lockup_authority_info, false)?;

        if stake_or_withdraw_authority_base_info.is_signer {
            signers.insert(Pubkey::create_with_seed(
                stake_or_withdraw_authority_base_info.key,
                &authorize_args.authority_seed,
                &authorize_args.authority_owner,
            )?);
        }

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
        let stake_account_info = next_account_info(account_info_iter)?;
        let rent_info = next_account_info(account_info_iter)?;
        let rent = &Rent::from_account_info(rent_info)?;
        let stake_authority_info = next_account_info(account_info_iter)?;
        let withdraw_authority_info = next_account_info(account_info_iter)?;

        if !withdraw_authority_info.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let authorized = Authorized {
            staker: *stake_authority_info.key,
            withdrawer: *withdraw_authority_info.key,
        };

        do_initialize(stake_account_info, authorized, Lockup::default(), rent)?;

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

        let (signers, custodian) = collect_signers(
            &[
                old_stake_or_withdraw_authority_info,
                new_stake_or_withdraw_authority_info,
            ],
            option_lockup_authority_info,
            true,
        )?;

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
        let stake_account_info = next_account_info(account_info_iter)?;
        let old_stake_or_withdraw_authority_base_info = next_account_info(account_info_iter)?;
        let clock_info = next_account_info(account_info_iter)?;
        let clock = &Clock::from_account_info(clock_info)?;
        let new_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let (mut signers, custodian) = collect_signers(
            &[new_stake_or_withdraw_authority_info],
            option_lockup_authority_info,
            true,
        )?;

        if old_stake_or_withdraw_authority_base_info.is_signer {
            signers.insert(Pubkey::create_with_seed(
                old_stake_or_withdraw_authority_base_info.key,
                &authorize_args.authority_seed,
                &authorize_args.authority_owner,
            )?);
        }

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
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let old_withdraw_or_lockup_authority_info = next_account_info(account_info_iter)?;
        let option_new_lockup_authority_info = next_account_info(account_info_iter).ok();
        let clock = Clock::get()?;

        let (signers, custodian) = collect_signers(
            &[old_withdraw_or_lockup_authority_info],
            option_new_lockup_authority_info,
            true,
        )?;

        let lockup = LockupArgs {
            unix_timestamp: lockup_checked.unix_timestamp,
            epoch: lockup_checked.epoch,
            custodian: custodian.copied(),
        };

        do_set_lockup(stake_account_info, signers, &lockup, &clock)?;

        Ok(())
    }

    fn process_deactivate_delinquent(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let stake_account_info = next_account_info(account_info_iter)?;
        let delinquent_vote_account_info = next_account_info(account_info_iter)?;
        let reference_vote_account_info = next_account_info(account_info_iter)?;
        let clock = Clock::get()?;

        let delinquent_vote_state = get_vote_state(delinquent_vote_account_info)?;
        let reference_vote_state = get_vote_state(reference_vote_account_info)?;

        if !acceptable_reference_epoch_credits(&reference_vote_state.epoch_credits, clock.epoch) {
            return Err(StakeError::InsufficientReferenceVotes.turn_into());
        }

        if let StakeStateV2::Stake(meta, mut stake, stake_flags) =
            get_stake_state(stake_account_info)?
        {
            if stake.delegation.voter_pubkey != *delinquent_vote_account_info.key {
                return Err(StakeError::VoteAddressMismatch.turn_into());
            }

            // Deactivate the stake account if its delegated vote account has never voted or has not
            // voted in the last `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`
            if eligible_for_deactivate_delinquent(&delinquent_vote_state.epoch_credits, clock.epoch)
            {
                stake
                    .deactivate(clock.epoch)
                    .map_err(StakeError::turn_into)?;

                set_stake_state(
                    stake_account_info,
                    &StakeStateV2::Stake(meta, stake, stake_flags),
                )
            } else {
                Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.turn_into())
            }
        } else {
            Err(ProgramError::InvalidAccountData)
        }?;

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
        //
        // XXX TODO FIXME remove neostake from the msg! commands
        // this is just so i can be sure its hitting the right program while testing
        match instruction {
            StakeInstruction::Initialize(authorize, lockup) => {
                msg!("NEOSTAKE Instruction: Initialize");
                Self::process_initialize(accounts, authorize, lockup)
            }
            StakeInstruction::Authorize(new_authority, authority_type) => {
                msg!("NEOSTAKE Instruction: Authorize");
                Self::process_authorize(accounts, new_authority, authority_type)
            }
            StakeInstruction::DelegateStake => {
                msg!("NEOSTAKE Instruction: DelegateStake");
                Self::process_delegate(accounts)
            }
            StakeInstruction::Split(lamports) => {
                msg!("NEOSTAKE Instruction: Split");
                Self::process_split(accounts, lamports)
            }
            StakeInstruction::Withdraw(lamports) => {
                msg!("NEOSTAKE Instruction: Withdraw");
                Self::process_withdraw(accounts, lamports)
            }
            StakeInstruction::Deactivate => {
                msg!("NEOSTAKE Instruction: Deactivate");
                Self::process_deactivate(accounts)
            }
            StakeInstruction::SetLockup(lockup) => {
                msg!("NEOSTAKE Instruction: SetLockup");
                Self::process_set_lockup(accounts, lockup)
            }
            StakeInstruction::Merge => {
                msg!("NEOSTAKE Instruction: Merge");
                Self::process_merge(accounts)
            }
            StakeInstruction::AuthorizeWithSeed(args) => {
                msg!("NEOSTAKE Instruction: AuthorizeWithSeed");
                Self::process_authorize_with_seed(accounts, args)
            }
            StakeInstruction::InitializeChecked => {
                msg!("NEOSTAKE Instruction: InitializeChecked");
                Self::process_initialize_checked(accounts)
            }
            StakeInstruction::AuthorizeChecked(authority_type) => {
                msg!("NEOSTAKE Instruction: AuthorizeChecked");
                Self::process_authorize_checked(accounts, authority_type)
            }
            StakeInstruction::AuthorizeCheckedWithSeed(args) => {
                msg!("NEOSTAKE Instruction: AuthorizeCheckedWithSeed");
                Self::process_authorize_checked_with_seed(accounts, args)
            }
            StakeInstruction::SetLockupChecked(lockup_checked) => {
                msg!("NEOSTAKE Instruction: SetLockup");
                Self::process_set_lockup_checked(accounts, lockup_checked)
            }
            StakeInstruction::GetMinimumDelegation => {
                msg!("NEOSTAKE Instruction: GetMinimumDelegation");
                let minimum_delegation = crate::get_minimum_delegation();
                set_return_data(&minimum_delegation.to_le_bytes());
                Ok(())
            }
            StakeInstruction::DeactivateDelinquent => {
                msg!("NEOSTAKE Instruction: DeactivateDelinquent");
                Self::process_deactivate_delinquent(accounts)
            }
            StakeInstruction::Redelegate => unimplemented!(), // wontfix
        }
    }
}
