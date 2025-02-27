/**
 * This code was AUTOGENERATED using the codama library.
 * Please DO NOT EDIT THIS FILE, instead use visitors
 * to add features, then rerun codama to update it.
 *
 * @see https://github.com/codama-idl/codama
 */

import {
  combineCodec,
  getStructDecoder,
  getStructEncoder,
  getU32Decoder,
  getU32Encoder,
  transformEncoder,
  type Address,
  type Codec,
  type Decoder,
  type Encoder,
  type IAccountMeta,
  type IAccountSignerMeta,
  type IInstruction,
  type IInstructionWithAccounts,
  type IInstructionWithData,
  type ReadonlyAccount,
  type ReadonlySignerAccount,
  type TransactionSigner,
  type WritableAccount,
} from '@solana/web3.js';
import { STAKE_PROGRAM_ADDRESS } from '../programs';
import { getAccountMetaFactory, type ResolvedAccount } from '../shared';

export const DEACTIVATE_DISCRIMINATOR = 5;

export function getDeactivateDiscriminatorBytes() {
  return getU32Encoder().encode(DEACTIVATE_DISCRIMINATOR);
}

export type DeactivateInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountStake extends string | IAccountMeta<string> = string,
  TAccountClockSysvar extends
    | string
    | IAccountMeta<string> = 'SysvarC1ock11111111111111111111111111111111',
  TAccountStakeAuthority extends string | IAccountMeta<string> = string,
  TRemainingAccounts extends readonly IAccountMeta<string>[] = [],
> = IInstruction<TProgram> &
  IInstructionWithData<Uint8Array> &
  IInstructionWithAccounts<
    [
      TAccountStake extends string
        ? WritableAccount<TAccountStake>
        : TAccountStake,
      TAccountClockSysvar extends string
        ? ReadonlyAccount<TAccountClockSysvar>
        : TAccountClockSysvar,
      TAccountStakeAuthority extends string
        ? ReadonlySignerAccount<TAccountStakeAuthority> &
            IAccountSignerMeta<TAccountStakeAuthority>
        : TAccountStakeAuthority,
      ...TRemainingAccounts,
    ]
  >;

export type DeactivateInstructionData = { discriminator: number };

export type DeactivateInstructionDataArgs = {};

export function getDeactivateInstructionDataEncoder(): Encoder<DeactivateInstructionDataArgs> {
  return transformEncoder(
    getStructEncoder([['discriminator', getU32Encoder()]]),
    (value) => ({ ...value, discriminator: DEACTIVATE_DISCRIMINATOR })
  );
}

export function getDeactivateInstructionDataDecoder(): Decoder<DeactivateInstructionData> {
  return getStructDecoder([['discriminator', getU32Decoder()]]);
}

export function getDeactivateInstructionDataCodec(): Codec<
  DeactivateInstructionDataArgs,
  DeactivateInstructionData
> {
  return combineCodec(
    getDeactivateInstructionDataEncoder(),
    getDeactivateInstructionDataDecoder()
  );
}

export type DeactivateInput<
  TAccountStake extends string = string,
  TAccountClockSysvar extends string = string,
  TAccountStakeAuthority extends string = string,
> = {
  /** Delegated stake account */
  stake: Address<TAccountStake>;
  /** Clock sysvar */
  clockSysvar?: Address<TAccountClockSysvar>;
  /** Stake authority */
  stakeAuthority: TransactionSigner<TAccountStakeAuthority>;
};

export function getDeactivateInstruction<
  TAccountStake extends string,
  TAccountClockSysvar extends string,
  TAccountStakeAuthority extends string,
  TProgramAddress extends Address = typeof STAKE_PROGRAM_ADDRESS,
>(
  input: DeactivateInput<
    TAccountStake,
    TAccountClockSysvar,
    TAccountStakeAuthority
  >,
  config?: { programAddress?: TProgramAddress }
): DeactivateInstruction<
  TProgramAddress,
  TAccountStake,
  TAccountClockSysvar,
  TAccountStakeAuthority
> {
  // Program address.
  const programAddress = config?.programAddress ?? STAKE_PROGRAM_ADDRESS;

  // Original accounts.
  const originalAccounts = {
    stake: { value: input.stake ?? null, isWritable: true },
    clockSysvar: { value: input.clockSysvar ?? null, isWritable: false },
    stakeAuthority: { value: input.stakeAuthority ?? null, isWritable: false },
  };
  const accounts = originalAccounts as Record<
    keyof typeof originalAccounts,
    ResolvedAccount
  >;

  // Resolve default values.
  if (!accounts.clockSysvar.value) {
    accounts.clockSysvar.value =
      'SysvarC1ock11111111111111111111111111111111' as Address<'SysvarC1ock11111111111111111111111111111111'>;
  }

  const getAccountMeta = getAccountMetaFactory(programAddress, 'programId');
  const instruction = {
    accounts: [
      getAccountMeta(accounts.stake),
      getAccountMeta(accounts.clockSysvar),
      getAccountMeta(accounts.stakeAuthority),
    ],
    programAddress,
    data: getDeactivateInstructionDataEncoder().encode({}),
  } as DeactivateInstruction<
    TProgramAddress,
    TAccountStake,
    TAccountClockSysvar,
    TAccountStakeAuthority
  >;

  return instruction;
}

export type ParsedDeactivateInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountMetas extends readonly IAccountMeta[] = readonly IAccountMeta[],
> = {
  programAddress: Address<TProgram>;
  accounts: {
    /** Delegated stake account */
    stake: TAccountMetas[0];
    /** Clock sysvar */
    clockSysvar: TAccountMetas[1];
    /** Stake authority */
    stakeAuthority: TAccountMetas[2];
  };
  data: DeactivateInstructionData;
};

export function parseDeactivateInstruction<
  TProgram extends string,
  TAccountMetas extends readonly IAccountMeta[],
>(
  instruction: IInstruction<TProgram> &
    IInstructionWithAccounts<TAccountMetas> &
    IInstructionWithData<Uint8Array>
): ParsedDeactivateInstruction<TProgram, TAccountMetas> {
  if (instruction.accounts.length < 3) {
    // TODO: Coded error.
    throw new Error('Not enough accounts');
  }
  let accountIndex = 0;
  const getNextAccount = () => {
    const accountMeta = instruction.accounts![accountIndex]!;
    accountIndex += 1;
    return accountMeta;
  };
  return {
    programAddress: instruction.programAddress,
    accounts: {
      stake: getNextAccount(),
      clockSysvar: getNextAccount(),
      stakeAuthority: getNextAccount(),
    },
    data: getDeactivateInstructionDataDecoder().decode(instruction.data),
  };
}
