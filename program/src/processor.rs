use {
    crate::{helpers::*, id, PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH},
    solana_account_info::{next_account_info, AccountInfo},
    solana_clock::Clock,
    solana_cpi::set_return_data,
    solana_msg::msg,
    solana_program_error::{ProgramError, ProgramResult},
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_stake_interface::{
        error::StakeError,
        instruction::{
            AuthorizeCheckedWithSeedArgs, AuthorizeWithSeedArgs, LockupArgs, LockupCheckedArgs,
            StakeInstruction,
        },
        stake_flags::StakeFlags,
        state::{Authorized, Lockup, Meta, StakeAuthorize, StakeStateV2},
        sysvar::stake_history::StakeHistorySysvar,
        tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
    },
    solana_sysvar::{epoch_rewards::EpochRewards, Sysvar},
    solana_sysvar_id::SysvarId,
    solana_vote_interface::{program as solana_vote_program, state::VoteStateV4},
    spherenet_validator_whitelist_interface::onchain::WhitelistEntryInformation,
    std::{collections::HashSet, mem::MaybeUninit},
};

fn get_vote_state(vote_account_info: &AccountInfo) -> Result<Box<VoteStateV4>, ProgramError> {
    if *vote_account_info.owner != solana_vote_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let mut vote_state = Box::new(MaybeUninit::uninit());
    VoteStateV4::deserialize_into_uninit(
        &vote_account_info.try_borrow_data()?,
        vote_state.as_mut(),
        vote_account_info.key,
    )
    .map_err(|_| ProgramError::InvalidAccountData)?;
    let vote_state = unsafe { vote_state.assume_init() };

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
) -> ProgramResult {
    if stake_account_info.data_len() != StakeStateV2::size_of() {
        return Err(ProgramError::InvalidAccountData);
    }

    if let StakeStateV2::Uninitialized = get_stake_state(stake_account_info)? {
        let rent = Rent::get()?;
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
) -> ProgramResult {
    let clock = &Clock::get()?;

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

// NOTE Our usage of the accounts iter is nonstandard, in imitation of the Native Stake Program.
// Native Stake typically, but not always, accumulated signers from the accounts array indiscriminately.
// This essentially allowed any account to act as a stake account signing authority.
// Instruction processors also asserted a required number of instruction accounts, often fewer than the actual number.
// When lengths were asserted in setup, accounts were retrieved via hardcoded index from `InstructionContext`,
// but after control was passed to main processing functions, they were pulled from the `TransactionContext`.
//
// When porting to BPF, we reimplemented this behavior exactly, such that both programs would be consensus-compatible:
// * All transactions that would fail on one program also fail on the other.
// * All transactions that would succeed on one program also succeed on the other.
// * For successful transactions, all account state transitions are identical.
// Error codes and log output sometimes differed.
//
// Native Stake also accepted some sysvars as input accounts, but pulled others from `InvokeContext`.
// This was done for backwards compatibility, but the end result was highly inconsistent.
//
// BPF Stake implements a new, stricter, interface, and supports both by branching when necessary.
// This new interface asserts that authorities are present in expected positions, and that they are signers.
// Self-signed stake accounts are still supported; the key simply must be passed in twice.
// The new interface also requires no sysvar accounts, retrieving all sysvars by syscall.
// Thus, we can fall back to the old interface if we encounter the first old-interface sysvar.
// Each processor has its own special logic, but we annotate "invariant," "diverge," and "converge" to make the flow obvious.
//
// We do not modify `Split`, `SetLockup`, and `SetLockupChecked`, as it would be a breaking change.
// These instructions never accepted sysvar accounts, so there is no way to distinguish "old" from "new."
// However, we may make this change if we determine there are no legitimate mainnet users of the lax constraints.
// Eventually, we may be able to remove the old interface and move to standard positional accounts for all instructions.
//
// New interface signer checks may duplicate later signer hashset checks. This is intended and harmless.
// `ok()` account retrievals (lockup custodians) were, are, and will always be optional by design.
pub struct Processor {}
impl Processor {
    fn process_initialize(
        accounts: &[AccountInfo],
        authorized: Authorized,
        lockup: Lockup,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;

        // `get_stake_state()` is called unconditionally, which checks owner
        do_initialize(stake_account_info, authorized, lockup)?;

        Ok(())
    }

    fn process_authorize(
        accounts: &[AccountInfo],
        new_authority: Pubkey,
        authority_type: StakeAuthorize,
    ) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;

        // diverge
        {
            let branch_account = next_account_info(account_info_iter)?;
            if Clock::check_id(branch_account.key) {
                let _stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
            } else {
                let stake_or_withdraw_authority_info = branch_account;
                if !stake_or_withdraw_authority_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
        }

        // converge
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

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
        )?;

        Ok(())
    }

    fn process_delegate(accounts: &[AccountInfo]) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;
        let vote_account_info = next_account_info(account_info_iter)?;

        // diverge
        {
            let branch_account = next_account_info(account_info_iter)?;
            if Clock::check_id(branch_account.key) {
                let _stake_history_info = next_account_info(account_info_iter)?;
                let whitelist_entry_info = next_account_info(account_info_iter)?;
                // let _stake_authority_info = next_account_info(account_info_iter);

                // Validate the validator vote account against whitelist entry
                let clock = &Clock::get()?;
                let whitelist_entry_information = WhitelistEntryInformation {
                    key: whitelist_entry_info.key,
                    owner: whitelist_entry_info.owner,
                    data: &whitelist_entry_info.try_borrow_data()?,
                };

                spherenet_validator_whitelist_interface::onchain::validate_vote_account_solana_program(
                    whitelist_entry_information,
                    vote_account_info.key,
                    &clock.epoch,
                )
                .map_err(|e| match e {
                    solana_instruction_error::InstructionError::Custom(code) => ProgramError::Custom(code),
                    _ => ProgramError::InvalidInstructionData,
                })?;
            } else {
                let stake_authority_info = branch_account;
                if !stake_authority_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
        };

        let clock = &Clock::get()?;
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
                    vote_state.credits(),
                    clock.epoch,
                );

                set_stake_state(
                    stake_account_info,
                    &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
                )
            }
            StakeStateV2::Stake(meta, mut stake, flags) => {
                // Only the staker may (re)delegate
                meta.authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(to_program_error)?;

                // Compute the maximum stake allowed to (re)delegate
                let ValidatedDelegatedInfo { stake_amount } =
                    validate_delegated_amount(stake_account_info, &meta)?;

                // Get current activation status at this epoch
                let effective_stake = stake.delegation.stake(
                    clock.epoch,
                    stake_history,
                    PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
                );

                if effective_stake == 0 {
                    // The stake has no effective voting power this epoch. This means it is either:
                    //   1. Inactive (fully cooled down after a previous deactivation)
                    //   2. Still activating (was delegated for the first time this epoch)
                    stake = new_stake(
                        stake_amount,
                        vote_account_info.key,
                        vote_state.credits(),
                        clock.epoch,
                    );
                } else if clock.epoch == stake.delegation.deactivation_epoch
                    && stake.delegation.voter_pubkey == *vote_account_info.key
                {
                    if stake_amount < stake.delegation.stake {
                        return Err(StakeError::InsufficientDelegation.into());
                    }
                    stake.delegation.deactivation_epoch = u64::MAX;
                } else {
                    // Not a valid state for redelegation
                    return Err(StakeError::TooSoonToRedelegate.into());
                }

                // Persist the updated stake state back to the account.
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

        // NOTE we cannot check this account without a breaking change
        // we may decide to enforce this if the pattern is not used on mainnet
        // let _stake_authority_info = next_account_info(account_info_iter);

        let rent = Rent::get()?;
        let clock = Clock::get()?;
        let stake_history = &StakeHistorySysvar(clock.epoch);
        let minimum_delegation = crate::get_minimum_delegation();

        if source_stake_account_info.key == destination_stake_account_info.key {
            return Err(ProgramError::InvalidArgument);
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

        if split_lamports == 0 {
            return Err(ProgramError::InsufficientFunds);
        }

        let destination_data_len = destination_stake_account_info.data_len();
        if destination_data_len != StakeStateV2::size_of() {
            return Err(ProgramError::InvalidAccountData);
        }
        let destination_rent_exempt_reserve = rent.minimum_balance(destination_data_len);

        // check signers and get delegation status along with a destination meta
        let source_stake_state = get_stake_state(source_stake_account_info)?;
        let (is_active_or_activating, option_dest_meta) = match source_stake_state {
            StakeStateV2::Stake(source_meta, source_stake, _) => {
                source_meta
                    .authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(to_program_error)?;

                let source_status = source_stake.delegation.stake_activating_and_deactivating(
                    clock.epoch,
                    stake_history,
                    PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
                );
                let is_active_or_activating =
                    source_status.effective > 0 || source_status.activating > 0;

                let mut dest_meta = source_meta;
                dest_meta.rent_exempt_reserve = destination_rent_exempt_reserve;

                (is_active_or_activating, Some(dest_meta))
            }
            StakeStateV2::Initialized(source_meta) => {
                source_meta
                    .authorized
                    .check(&signers, StakeAuthorize::Staker)
                    .map_err(to_program_error)?;

                let mut dest_meta = source_meta;
                dest_meta.rent_exempt_reserve = destination_rent_exempt_reserve;

                (false, Some(dest_meta))
            }
            StakeStateV2::Uninitialized => {
                if !source_stake_account_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }

                (false, None)
            }
            StakeStateV2::RewardsPool => return Err(ProgramError::InvalidAccountData),
        };

        // special case: for a full split, we only care that the destination becomes a valid stake account
        // this prevents state changes in exceptional cases where a once-valid source has become invalid
        // relocate lamports, copy data, and close the original account
        if split_lamports == source_lamport_balance {
            let mut destination_stake_state = source_stake_state;
            let delegation = match (&mut destination_stake_state, option_dest_meta) {
                (StakeStateV2::Stake(meta, stake, _), Some(dest_meta)) => {
                    *meta = dest_meta;

                    if is_active_or_activating {
                        stake.delegation.stake
                    } else {
                        0
                    }
                }
                (StakeStateV2::Initialized(meta), Some(dest_meta)) => {
                    *meta = dest_meta;

                    0
                }
                (StakeStateV2::Uninitialized, None) => 0,
                _ => unreachable!(),
            };

            if destination_lamport_balance
                .saturating_add(split_lamports)
                .saturating_sub(delegation)
                < destination_rent_exempt_reserve
            {
                return Err(ProgramError::InsufficientFunds);
            }

            if is_active_or_activating && delegation < minimum_delegation {
                return Err(StakeError::InsufficientDelegation.into());
            }

            set_stake_state(destination_stake_account_info, &destination_stake_state)?;
            source_stake_account_info.resize(0)?;

            relocate_lamports(
                source_stake_account_info,
                destination_stake_account_info,
                split_lamports,
            )?;

            return Ok(());
        }

        // special case: if stake is fully inactive, we only care that both accounts meet rent-exemption
        if !is_active_or_activating {
            let source_rent_exempt_reserve =
                rent.minimum_balance(source_stake_account_info.data_len());

            let mut destination_stake_state = source_stake_state;
            match (&mut destination_stake_state, option_dest_meta) {
                (StakeStateV2::Stake(meta, _, _), Some(dest_meta))
                | (StakeStateV2::Initialized(meta), Some(dest_meta)) => {
                    *meta = dest_meta;
                }
                (StakeStateV2::Uninitialized, None) => (),
                _ => unreachable!(),
            }

            let post_source_lamports = source_lamport_balance
                .checked_sub(split_lamports)
                .ok_or(ProgramError::InsufficientFunds)?;

            let post_destination_lamports = destination_lamport_balance
                .checked_add(split_lamports)
                .ok_or(ProgramError::ArithmeticOverflow)?;

            if post_source_lamports < source_rent_exempt_reserve
                || post_destination_lamports < destination_rent_exempt_reserve
            {
                return Err(ProgramError::InsufficientFunds);
            }

            set_stake_state(destination_stake_account_info, &destination_stake_state)?;

            relocate_lamports(
                source_stake_account_info,
                destination_stake_account_info,
                split_lamports,
            )?;

            return Ok(());
        }

        // at this point, we know we have a StakeStateV2::Stake source that is either activating or has nonzero effective
        // this means we must redistribute the delegation across both accounts and enforce:
        // * destination has a pre-funded rent exemption
        // * source meets rent exemption less its remaining delegation
        // * source and destination both meet the minimum delegation
        // destination delegation is matched 1:1 by split lamports. in other words, free source lamports are never split
        match (source_stake_state, option_dest_meta) {
            (StakeStateV2::Stake(source_meta, mut source_stake, stake_flags), Some(dest_meta)) => {
                if destination_lamport_balance < destination_rent_exempt_reserve {
                    return Err(ProgramError::InsufficientFunds);
                }

                let mut dest_stake = source_stake;

                source_stake.delegation.stake = source_stake
                    .delegation
                    .stake
                    .checked_sub(split_lamports)
                    .ok_or::<ProgramError>(StakeError::InsufficientDelegation.into())?;

                if source_stake.delegation.stake < minimum_delegation {
                    return Err(StakeError::InsufficientDelegation.into());
                }

                // sanity check on prior math; this branch is unreachable
                // minimum delegation is by definition nonzero, and we remove one delegated lamport per split lamport
                // since the remaining source delegation > 0, it is impossible that we took from its rent-exempt reserve
                if source_lamport_balance
                    .saturating_sub(split_lamports)
                    .saturating_sub(source_stake.delegation.stake)
                    < source_meta.rent_exempt_reserve
                {
                    return Err(ProgramError::InsufficientFunds);
                }

                dest_stake.delegation.stake = split_lamports;
                if dest_stake.delegation.stake < minimum_delegation {
                    return Err(StakeError::InsufficientDelegation.into());
                }

                set_stake_state(
                    source_stake_account_info,
                    &StakeStateV2::Stake(source_meta, source_stake, stake_flags),
                )?;

                set_stake_state(
                    destination_stake_account_info,
                    &StakeStateV2::Stake(dest_meta, dest_stake, stake_flags),
                )?;

                relocate_lamports(
                    source_stake_account_info,
                    destination_stake_account_info,
                    split_lamports,
                )?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn process_withdraw(accounts: &[AccountInfo], withdraw_lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // invariant
        let source_stake_account_info = next_account_info(account_info_iter)?;
        let destination_info = next_account_info(account_info_iter)?;

        // diverge
        let withdraw_authority_info = {
            let branch_account = next_account_info(account_info_iter)?;
            if Clock::check_id(branch_account.key) {
                let _stake_history_info = next_account_info(account_info_iter)?;
                next_account_info(account_info_iter)?
            } else {
                let withdraw_authority_info = branch_account;
                if !withdraw_authority_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
                withdraw_authority_info
            }
        };

        // converge
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

        let clock = &Clock::get()?;
        let stake_history = &StakeHistorySysvar(clock.epoch);

        if source_stake_account_info.key == destination_info.key {
            return Err(ProgramError::InvalidArgument);
        }

        // this is somewhat subtle. for Initialized and Stake, there is a real authority
        // but for Uninitialized, the source account is passed twice, and signed for
        let (signers, custodian) =
            collect_signers_checked(Some(withdraw_authority_info), option_lockup_authority_info)?;

        let (lockup, reserve, is_staked) = match get_stake_state(source_stake_account_info) {
            Ok(StakeStateV2::Stake(meta, stake, _stake_flag)) => {
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
            Ok(StakeStateV2::Initialized(meta)) => {
                meta.authorized
                    .check(&signers, StakeAuthorize::Withdrawer)
                    .map_err(to_program_error)?;
                // stake accounts must have a balance >= rent_exempt_reserve
                (meta.lockup, meta.rent_exempt_reserve, false)
            }
            Ok(StakeStateV2::Uninitialized) => {
                if !signers.contains(source_stake_account_info.key) {
                    return Err(ProgramError::MissingRequiredSignature);
                }
                (Lockup::default(), 0, false) // no lockup, no restrictions
            }
            Err(e)
                if e == ProgramError::InvalidAccountData
                    && source_stake_account_info.data_len() == 0 =>
            {
                if !signers.contains(source_stake_account_info.key) {
                    return Err(ProgramError::MissingRequiredSignature);
                }
                (Lockup::default(), 0, false) // no lockup, no restrictions
            }
            Ok(StakeStateV2::RewardsPool) => return Err(ProgramError::InvalidAccountData),
            Err(e) => return Err(e),
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

            // Truncate state upon zero balance
            source_stake_account_info.resize(0)?;
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

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;

        // diverge
        {
            let branch_account = next_account_info(account_info_iter)?;
            if Clock::check_id(branch_account.key) {
                // let _stake_authority_info = next_account_info(account_info_iter);
            } else {
                let stake_authority_info = branch_account;
                if !stake_authority_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
        }

        let clock = &Clock::get()?;

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

        // NOTE we cannot check this account without a breaking change
        // we may decide to enforce this if the pattern is not used on mainnet
        // let _old_withdraw_or_lockup_authority_info = next_account_info(account_info_iter);

        let clock = Clock::get()?;

        // `get_stake_state()` is called unconditionally, which checks owner
        do_set_lockup(stake_account_info, &signers, &lockup, &clock)?;

        Ok(())
    }

    fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // invariant
        let destination_stake_account_info = next_account_info(account_info_iter)?;
        let source_stake_account_info = next_account_info(account_info_iter)?;

        // diverge
        {
            let branch_account = next_account_info(account_info_iter)?;
            if Clock::check_id(branch_account.key) {
                let _stake_history_info = next_account_info(account_info_iter)?;
                // let _stake_authority_info = next_account_info(account_info_iter);
            } else {
                let stake_authority_info = branch_account;
                if !stake_authority_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
        }

        let clock = &Clock::get()?;
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

        // Source is about to be drained, truncate its state
        source_stake_account_info.resize(0)?;

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

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;
        let stake_or_withdraw_authority_base_info = next_account_info(account_info_iter)?;

        // diverge
        let option_lockup_authority_info = {
            let branch_account = next_account_info(account_info_iter).ok();
            if branch_account.is_some_and(|account| Clock::check_id(account.key)) {
                next_account_info(account_info_iter).ok()
            } else {
                branch_account
            }
        };

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
        )?;

        Ok(())
    }

    fn process_initialize_checked(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;

        // diverge
        let stake_authority_info = {
            let branch_account = next_account_info(account_info_iter)?;
            if Rent::check_id(branch_account.key) {
                next_account_info(account_info_iter)?
            } else {
                // we do not need to check this, withdraw_authority is the only signer
                branch_account
            }
        };

        // converge
        let withdraw_authority_info = next_account_info(account_info_iter)?;

        if !withdraw_authority_info.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let authorized = Authorized {
            staker: *stake_authority_info.key,
            withdrawer: *withdraw_authority_info.key,
        };

        // `get_stake_state()` is called unconditionally, which checks owner
        do_initialize(stake_account_info, authorized, Lockup::default())?;

        Ok(())
    }

    fn process_authorize_checked(
        accounts: &[AccountInfo],
        authority_type: StakeAuthorize,
    ) -> ProgramResult {
        let signers = collect_signers(accounts);
        let account_info_iter = &mut accounts.iter();

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;

        // diverge
        {
            let branch_account = next_account_info(account_info_iter)?;
            if Clock::check_id(branch_account.key) {
                let _old_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
            } else {
                let old_stake_or_withdraw_authority_info = branch_account;
                if !old_stake_or_withdraw_authority_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
        }

        // converge
        let new_stake_or_withdraw_authority_info = next_account_info(account_info_iter)?;
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

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
        )?;

        Ok(())
    }

    fn process_authorize_checked_with_seed(
        accounts: &[AccountInfo],
        authorize_args: AuthorizeCheckedWithSeedArgs,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;
        let old_stake_or_withdraw_authority_base_info = next_account_info(account_info_iter)?;

        // diverge
        let new_stake_or_withdraw_authority_info = {
            let branch_account = next_account_info(account_info_iter)?;
            if Clock::check_id(branch_account.key) {
                next_account_info(account_info_iter)?
            } else {
                let new_stake_or_withdraw_authority_info = branch_account;
                if !new_stake_or_withdraw_authority_info.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
                new_stake_or_withdraw_authority_info
            }
        };

        // converge
        let option_lockup_authority_info = next_account_info(account_info_iter).ok();

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

        // NOTE we cannot check this account without a breaking change
        // we may decide to enforce this if the pattern is not used on mainnet
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

    fn deactivate_network_delinquent(
        stake_account_info: &AccountInfo,
        delinquent_vote_account_info: &AccountInfo,
        reference_vote_account_info: &AccountInfo,
        clock: &Clock,
    ) -> ProgramResult {
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
        }
    }

    fn deactivate_delisted_delinquent(
        stake_account_info: &AccountInfo,
        delisted_vote_account_info: &AccountInfo,
        whitelist_entry_account_info: &AccountInfo,
        clock: &Clock,
    ) -> ProgramResult {
        // Verify delisted vote account is a vote account
        if *delisted_vote_account_info.owner != solana_vote_program::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        // Validate whitelist entry
        let whitelist_entry_information = WhitelistEntryInformation {
            key: whitelist_entry_account_info.key,
            owner: whitelist_entry_account_info.owner,
            data: &whitelist_entry_account_info.try_borrow_data()?,
        };

        if let StakeStateV2::Stake(meta, mut stake, stake_flags) =
            get_stake_state(stake_account_info)?
        {
            if stake.delegation.voter_pubkey != *delisted_vote_account_info.key {
                return Err(StakeError::VoteAddressMismatch.into());
            }

            // Verify validator is delisted (whitelist entry is system-owned tombstone)
            spherenet_validator_whitelist_interface::onchain::throw_if_not_unlisted(
                whitelist_entry_information,
                delisted_vote_account_info.key,
            )
            .map_err(|e| match e {
                solana_instruction_error::InstructionError::Custom(code) => ProgramError::Custom(code),
                _ => ProgramError::InvalidInstructionData,
            })?;

            stake.deactivate(clock.epoch)?;

            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, stake_flags),
            )
        } else {
            Err(ProgramError::InvalidAccountData)
        }
    }

    fn process_deactivate_delinquent(accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // invariant
        let stake_account_info = next_account_info(account_info_iter)?;
        let delinquent_vote_account_info = next_account_info(account_info_iter)?;
        let support_account_info = next_account_info(account_info_iter)?;

        let clock = Clock::get()?;

        // Dispatch based on support account owner
        match *support_account_info.owner {
            owner if owner == solana_vote_program::id() => {
                // Network delinquent: reference vote account
                Self::deactivate_network_delinquent(
                    stake_account_info,
                    delinquent_vote_account_info,
                    support_account_info,
                    &clock,
                )
            }
            owner if owner == spherenet_validator_whitelist_interface::program_solana::id()
                || owner == solana_sdk_ids::system_program::id() => {
                // Delisted delinquent: whitelist entry (active or tombstone)
                Self::deactivate_delisted_delinquent(
                    stake_account_info,
                    delinquent_vote_account_info,
                    support_account_info,
                    &clock,
                )
            }
            _ => {
                // Reject unknown account types
                Err(ProgramError::IncorrectProgramId)
            }
        }
    }

    fn process_move_stake(accounts: &[AccountInfo], lamports: u64) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        // invariant
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

        // invariant
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
