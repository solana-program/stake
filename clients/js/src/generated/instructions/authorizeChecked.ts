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
import {
  getStakeAuthorizeDecoder,
  getStakeAuthorizeEncoder,
  type StakeAuthorize,
  type StakeAuthorizeArgs,
} from '../types';

export const AUTHORIZE_CHECKED_DISCRIMINATOR = 10;

export function getAuthorizeCheckedDiscriminatorBytes() {
  return getU32Encoder().encode(AUTHORIZE_CHECKED_DISCRIMINATOR);
}

export type AuthorizeCheckedInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountStake extends string | IAccountMeta<string> = string,
  TAccountClockSysvar extends
    | string
    | IAccountMeta<string> = 'SysvarC1ock11111111111111111111111111111111',
  TAccountAuthority extends string | IAccountMeta<string> = string,
  TAccountNewAuthority extends string | IAccountMeta<string> = string,
  TAccountLockupAuthority extends string | IAccountMeta<string> = string,
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
      TAccountAuthority extends string
        ? ReadonlySignerAccount<TAccountAuthority> &
            IAccountSignerMeta<TAccountAuthority>
        : TAccountAuthority,
      TAccountNewAuthority extends string
        ? ReadonlySignerAccount<TAccountNewAuthority> &
            IAccountSignerMeta<TAccountNewAuthority>
        : TAccountNewAuthority,
      TAccountLockupAuthority extends string
        ? ReadonlySignerAccount<TAccountLockupAuthority> &
            IAccountSignerMeta<TAccountLockupAuthority>
        : TAccountLockupAuthority,
      ...TRemainingAccounts,
    ]
  >;

export type AuthorizeCheckedInstructionData = {
  discriminator: number;
  stakeAuthorize: StakeAuthorize;
};

export type AuthorizeCheckedInstructionDataArgs = {
  stakeAuthorize: StakeAuthorizeArgs;
};

export function getAuthorizeCheckedInstructionDataEncoder(): Encoder<AuthorizeCheckedInstructionDataArgs> {
  return transformEncoder(
    getStructEncoder([
      ['discriminator', getU32Encoder()],
      ['stakeAuthorize', getStakeAuthorizeEncoder()],
    ]),
    (value) => ({ ...value, discriminator: AUTHORIZE_CHECKED_DISCRIMINATOR })
  );
}

export function getAuthorizeCheckedInstructionDataDecoder(): Decoder<AuthorizeCheckedInstructionData> {
  return getStructDecoder([
    ['discriminator', getU32Decoder()],
    ['stakeAuthorize', getStakeAuthorizeDecoder()],
  ]);
}

export function getAuthorizeCheckedInstructionDataCodec(): Codec<
  AuthorizeCheckedInstructionDataArgs,
  AuthorizeCheckedInstructionData
> {
  return combineCodec(
    getAuthorizeCheckedInstructionDataEncoder(),
    getAuthorizeCheckedInstructionDataDecoder()
  );
}

export type AuthorizeCheckedInput<
  TAccountStake extends string = string,
  TAccountClockSysvar extends string = string,
  TAccountAuthority extends string = string,
  TAccountNewAuthority extends string = string,
  TAccountLockupAuthority extends string = string,
> = {
  /** Stake account to be updated */
  stake: Address<TAccountStake>;
  /** Clock sysvar */
  clockSysvar?: Address<TAccountClockSysvar>;
  /** The stake or withdraw authority */
  authority: TransactionSigner<TAccountAuthority>;
  /** The new stake or withdraw authority */
  newAuthority: TransactionSigner<TAccountNewAuthority>;
  /** Lockup authority */
  lockupAuthority?: TransactionSigner<TAccountLockupAuthority>;
  stakeAuthorize: AuthorizeCheckedInstructionDataArgs['stakeAuthorize'];
};

export function getAuthorizeCheckedInstruction<
  TAccountStake extends string,
  TAccountClockSysvar extends string,
  TAccountAuthority extends string,
  TAccountNewAuthority extends string,
  TAccountLockupAuthority extends string,
  TProgramAddress extends Address = typeof STAKE_PROGRAM_ADDRESS,
>(
  input: AuthorizeCheckedInput<
    TAccountStake,
    TAccountClockSysvar,
    TAccountAuthority,
    TAccountNewAuthority,
    TAccountLockupAuthority
  >,
  config?: { programAddress?: TProgramAddress }
): AuthorizeCheckedInstruction<
  TProgramAddress,
  TAccountStake,
  TAccountClockSysvar,
  TAccountAuthority,
  TAccountNewAuthority,
  TAccountLockupAuthority
> {
  // Program address.
  const programAddress = config?.programAddress ?? STAKE_PROGRAM_ADDRESS;

  // Original accounts.
  const originalAccounts = {
    stake: { value: input.stake ?? null, isWritable: true },
    clockSysvar: { value: input.clockSysvar ?? null, isWritable: false },
    authority: { value: input.authority ?? null, isWritable: false },
    newAuthority: { value: input.newAuthority ?? null, isWritable: false },
    lockupAuthority: {
      value: input.lockupAuthority ?? null,
      isWritable: false,
    },
  };
  const accounts = originalAccounts as Record<
    keyof typeof originalAccounts,
    ResolvedAccount
  >;

  // Original args.
  const args = { ...input };

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
      getAccountMeta(accounts.authority),
      getAccountMeta(accounts.newAuthority),
      getAccountMeta(accounts.lockupAuthority),
    ],
    programAddress,
    data: getAuthorizeCheckedInstructionDataEncoder().encode(
      args as AuthorizeCheckedInstructionDataArgs
    ),
  } as AuthorizeCheckedInstruction<
    TProgramAddress,
    TAccountStake,
    TAccountClockSysvar,
    TAccountAuthority,
    TAccountNewAuthority,
    TAccountLockupAuthority
  >;

  return instruction;
}

export type ParsedAuthorizeCheckedInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountMetas extends readonly IAccountMeta[] = readonly IAccountMeta[],
> = {
  programAddress: Address<TProgram>;
  accounts: {
    /** Stake account to be updated */
    stake: TAccountMetas[0];
    /** Clock sysvar */
    clockSysvar: TAccountMetas[1];
    /** The stake or withdraw authority */
    authority: TAccountMetas[2];
    /** The new stake or withdraw authority */
    newAuthority: TAccountMetas[3];
    /** Lockup authority */
    lockupAuthority?: TAccountMetas[4] | undefined;
  };
  data: AuthorizeCheckedInstructionData;
};

export function parseAuthorizeCheckedInstruction<
  TProgram extends string,
  TAccountMetas extends readonly IAccountMeta[],
>(
  instruction: IInstruction<TProgram> &
    IInstructionWithAccounts<TAccountMetas> &
    IInstructionWithData<Uint8Array>
): ParsedAuthorizeCheckedInstruction<TProgram, TAccountMetas> {
  if (instruction.accounts.length < 5) {
    // TODO: Coded error.
    throw new Error('Not enough accounts');
  }
  let accountIndex = 0;
  const getNextAccount = () => {
    const accountMeta = instruction.accounts![accountIndex]!;
    accountIndex += 1;
    return accountMeta;
  };
  const getNextOptionalAccount = () => {
    const accountMeta = getNextAccount();
    return accountMeta.address === STAKE_PROGRAM_ADDRESS
      ? undefined
      : accountMeta;
  };
  return {
    programAddress: instruction.programAddress,
    accounts: {
      stake: getNextAccount(),
      clockSysvar: getNextAccount(),
      authority: getNextAccount(),
      newAuthority: getNextAccount(),
      lockupAuthority: getNextOptionalAccount(),
    },
    data: getAuthorizeCheckedInstructionDataDecoder().decode(instruction.data),
  };
}
