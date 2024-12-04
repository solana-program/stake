/**
 * This code was AUTOGENERATED using the codama library.
 * Please DO NOT EDIT THIS FILE, instead use visitors
 * to add features, then rerun codama to update it.
 *
 * @see https://github.com/codama-idl/codama
 */

import {
  containsBytes,
  getU8Encoder,
  type Address,
  type ReadonlyUint8Array,
} from '@solana/web3.js';
import {
  type ParsedAuthorizeCheckedInstruction,
  type ParsedAuthorizeCheckedWithSeedInstruction,
  type ParsedAuthorizeInstruction,
  type ParsedAuthorizeWithSeedInstruction,
  type ParsedDeactivateDelinquentInstruction,
  type ParsedDeactivateInstruction,
  type ParsedDelegateStakeInstruction,
  type ParsedGetMinimumDelegationInstruction,
  type ParsedInitializeCheckedInstruction,
  type ParsedInitializeInstruction,
  type ParsedMergeInstruction,
  type ParsedMoveLamportsInstruction,
  type ParsedMoveStakeInstruction,
  type ParsedSetLockupCheckedInstruction,
  type ParsedSetLockupInstruction,
  type ParsedSplitInstruction,
  type ParsedWithdrawInstruction,
} from '../instructions';

export const STAKE_PROGRAM_ADDRESS =
  'Stake11111111111111111111111111111111111111' as Address<'Stake11111111111111111111111111111111111111'>;

export enum StakeAccount {
  StakeAccount,
}

export enum StakeInstruction {
  Initialize,
  Authorize,
  DelegateStake,
  Split,
  Withdraw,
  Deactivate,
  SetLockup,
  Merge,
  AuthorizeWithSeed,
  InitializeChecked,
  AuthorizeChecked,
  AuthorizeCheckedWithSeed,
  SetLockupChecked,
  GetMinimumDelegation,
  DeactivateDelinquent,
  MoveStake,
  MoveLamports,
}

export function identifyStakeInstruction(
  instruction: { data: ReadonlyUint8Array } | ReadonlyUint8Array
): StakeInstruction {
  const data = 'data' in instruction ? instruction.data : instruction;
  if (containsBytes(data, getU8Encoder().encode(0), 0)) {
    return StakeInstruction.Initialize;
  }
  if (containsBytes(data, getU8Encoder().encode(1), 0)) {
    return StakeInstruction.Authorize;
  }
  if (containsBytes(data, getU8Encoder().encode(2), 0)) {
    return StakeInstruction.DelegateStake;
  }
  if (containsBytes(data, getU8Encoder().encode(3), 0)) {
    return StakeInstruction.Split;
  }
  if (containsBytes(data, getU8Encoder().encode(4), 0)) {
    return StakeInstruction.Withdraw;
  }
  if (containsBytes(data, getU8Encoder().encode(5), 0)) {
    return StakeInstruction.Deactivate;
  }
  if (containsBytes(data, getU8Encoder().encode(6), 0)) {
    return StakeInstruction.SetLockup;
  }
  if (containsBytes(data, getU8Encoder().encode(7), 0)) {
    return StakeInstruction.Merge;
  }
  if (containsBytes(data, getU8Encoder().encode(8), 0)) {
    return StakeInstruction.AuthorizeWithSeed;
  }
  if (containsBytes(data, getU8Encoder().encode(9), 0)) {
    return StakeInstruction.InitializeChecked;
  }
  if (containsBytes(data, getU8Encoder().encode(10), 0)) {
    return StakeInstruction.AuthorizeChecked;
  }
  if (containsBytes(data, getU8Encoder().encode(11), 0)) {
    return StakeInstruction.AuthorizeCheckedWithSeed;
  }
  if (containsBytes(data, getU8Encoder().encode(12), 0)) {
    return StakeInstruction.SetLockupChecked;
  }
  if (containsBytes(data, getU8Encoder().encode(13), 0)) {
    return StakeInstruction.GetMinimumDelegation;
  }
  if (containsBytes(data, getU8Encoder().encode(14), 0)) {
    return StakeInstruction.DeactivateDelinquent;
  }
  if (containsBytes(data, getU8Encoder().encode(16), 0)) {
    return StakeInstruction.MoveStake;
  }
  if (containsBytes(data, getU8Encoder().encode(17), 0)) {
    return StakeInstruction.MoveLamports;
  }
  throw new Error(
    'The provided instruction could not be identified as a stake instruction.'
  );
}

export type ParsedStakeInstruction<
  TProgram extends string = 'Stake11111111111111111111111111111111111111',
> =
  | ({
      instructionType: StakeInstruction.Initialize;
    } & ParsedInitializeInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.Authorize;
    } & ParsedAuthorizeInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.DelegateStake;
    } & ParsedDelegateStakeInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.Split;
    } & ParsedSplitInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.Withdraw;
    } & ParsedWithdrawInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.Deactivate;
    } & ParsedDeactivateInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.SetLockup;
    } & ParsedSetLockupInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.Merge;
    } & ParsedMergeInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.AuthorizeWithSeed;
    } & ParsedAuthorizeWithSeedInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.InitializeChecked;
    } & ParsedInitializeCheckedInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.AuthorizeChecked;
    } & ParsedAuthorizeCheckedInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.AuthorizeCheckedWithSeed;
    } & ParsedAuthorizeCheckedWithSeedInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.SetLockupChecked;
    } & ParsedSetLockupCheckedInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.GetMinimumDelegation;
    } & ParsedGetMinimumDelegationInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.DeactivateDelinquent;
    } & ParsedDeactivateDelinquentInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.MoveStake;
    } & ParsedMoveStakeInstruction<TProgram>)
  | ({
      instructionType: StakeInstruction.MoveLamports;
    } & ParsedMoveLamportsInstruction<TProgram>);
