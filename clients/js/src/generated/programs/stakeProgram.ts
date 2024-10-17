/**
 * This code was AUTOGENERATED using the codama library.
 * Please DO NOT EDIT THIS FILE, instead use visitors
 * to add features, then rerun codama to update it.
 *
 * @see https://github.com/codama-idl/codama
 */

import {
  containsBytes,
  fixEncoderSize,
  getBytesEncoder,
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
  type ParsedSetLockupCheckedInstruction,
  type ParsedSetLockupInstruction,
  type ParsedSplitInstruction,
  type ParsedWithdrawInstruction,
} from '../instructions';

export const STAKE_PROGRAM_PROGRAM_ADDRESS =
  'Stake11111111111111111111111111111111111111' as Address<'Stake11111111111111111111111111111111111111'>;

export enum StakeProgramInstruction {
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
}

export function identifyStakeProgramInstruction(
  instruction: { data: ReadonlyUint8Array } | ReadonlyUint8Array
): StakeProgramInstruction {
  const data = 'data' in instruction ? instruction.data : instruction;
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([175, 175, 109, 31, 13, 152, 155, 237])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.Initialize;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([173, 193, 102, 210, 219, 137, 113, 120])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.Authorize;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([50, 110, 95, 179, 194, 75, 140, 246])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.DelegateStake;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([124, 189, 27, 43, 216, 40, 147, 66])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.Split;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([183, 18, 70, 156, 148, 109, 161, 34])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.Withdraw;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([44, 112, 33, 172, 113, 28, 142, 13])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.Deactivate;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([44, 170, 189, 40, 128, 123, 252, 201])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.SetLockup;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([148, 141, 236, 47, 174, 126, 69, 111])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.Merge;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([7, 18, 211, 41, 76, 83, 115, 61])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.AuthorizeWithSeed;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([219, 90, 58, 161, 139, 88, 246, 28])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.InitializeChecked;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([147, 97, 67, 26, 230, 107, 45, 242])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.AuthorizeChecked;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([14, 230, 154, 165, 225, 209, 194, 210])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.AuthorizeCheckedWithSeed;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([22, 158, 12, 183, 118, 94, 156, 255])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.SetLockupChecked;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([197, 65, 7, 73, 151, 105, 133, 105])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.GetMinimumDelegation;
  }
  if (
    containsBytes(
      data,
      fixEncoderSize(getBytesEncoder(), 8).encode(
        new Uint8Array([6, 113, 198, 138, 228, 163, 159, 221])
      ),
      0
    )
  ) {
    return StakeProgramInstruction.DeactivateDelinquent;
  }
  throw new Error(
    'The provided instruction could not be identified as a stakeProgram instruction.'
  );
}

export type ParsedStakeProgramInstruction<
  TProgram extends string = 'Stake11111111111111111111111111111111111111',
> =
  | ({
      instructionType: StakeProgramInstruction.Initialize;
    } & ParsedInitializeInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.Authorize;
    } & ParsedAuthorizeInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.DelegateStake;
    } & ParsedDelegateStakeInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.Split;
    } & ParsedSplitInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.Withdraw;
    } & ParsedWithdrawInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.Deactivate;
    } & ParsedDeactivateInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.SetLockup;
    } & ParsedSetLockupInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.Merge;
    } & ParsedMergeInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.AuthorizeWithSeed;
    } & ParsedAuthorizeWithSeedInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.InitializeChecked;
    } & ParsedInitializeCheckedInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.AuthorizeChecked;
    } & ParsedAuthorizeCheckedInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.AuthorizeCheckedWithSeed;
    } & ParsedAuthorizeCheckedWithSeedInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.SetLockupChecked;
    } & ParsedSetLockupCheckedInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.GetMinimumDelegation;
    } & ParsedGetMinimumDelegationInstruction<TProgram>)
  | ({
      instructionType: StakeProgramInstruction.DeactivateDelinquent;
    } & ParsedDeactivateDelinquentInstruction<TProgram>);
