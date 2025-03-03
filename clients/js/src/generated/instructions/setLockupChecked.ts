/**
 * This code was AUTOGENERATED using the codama library.
 * Please DO NOT EDIT THIS FILE, instead use visitors
 * to add features, then rerun codama to update it.
 *
 * @see https://github.com/codama-idl/codama
 */

import {
  combineCodec,
  getI64Decoder,
  getI64Encoder,
  getOptionDecoder,
  getOptionEncoder,
  getStructDecoder,
  getStructEncoder,
  getU32Decoder,
  getU32Encoder,
  getU64Decoder,
  getU64Encoder,
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
  type Option,
  type OptionOrNullable,
  type ReadonlySignerAccount,
  type TransactionSigner,
  type WritableAccount,
} from '@solana/kit';
import { STAKE_PROGRAM_ADDRESS } from '../programs';
import { getAccountMetaFactory, type ResolvedAccount } from '../shared';

export const SET_LOCKUP_CHECKED_DISCRIMINATOR = 12;

export function getSetLockupCheckedDiscriminatorBytes() {
  return getU32Encoder().encode(SET_LOCKUP_CHECKED_DISCRIMINATOR);
}

export type SetLockupCheckedInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountStake extends string | IAccountMeta<string> = string,
  TAccountAuthority extends string | IAccountMeta<string> = string,
  TAccountNewAuthority extends string | IAccountMeta<string> = string,
  TRemainingAccounts extends readonly IAccountMeta<string>[] = [],
> = IInstruction<TProgram> &
  IInstructionWithData<Uint8Array> &
  IInstructionWithAccounts<
    [
      TAccountStake extends string
        ? WritableAccount<TAccountStake>
        : TAccountStake,
      TAccountAuthority extends string
        ? ReadonlySignerAccount<TAccountAuthority> &
            IAccountSignerMeta<TAccountAuthority>
        : TAccountAuthority,
      TAccountNewAuthority extends string
        ? ReadonlySignerAccount<TAccountNewAuthority> &
            IAccountSignerMeta<TAccountNewAuthority>
        : TAccountNewAuthority,
      ...TRemainingAccounts,
    ]
  >;

export type SetLockupCheckedInstructionData = {
  discriminator: number;
  unixTimestamp: Option<bigint>;
  epoch: Option<bigint>;
};

export type SetLockupCheckedInstructionDataArgs = {
  unixTimestamp: OptionOrNullable<number | bigint>;
  epoch: OptionOrNullable<number | bigint>;
};

export function getSetLockupCheckedInstructionDataEncoder(): Encoder<SetLockupCheckedInstructionDataArgs> {
  return transformEncoder(
    getStructEncoder([
      ['discriminator', getU32Encoder()],
      ['unixTimestamp', getOptionEncoder(getI64Encoder())],
      ['epoch', getOptionEncoder(getU64Encoder())],
    ]),
    (value) => ({ ...value, discriminator: SET_LOCKUP_CHECKED_DISCRIMINATOR })
  );
}

export function getSetLockupCheckedInstructionDataDecoder(): Decoder<SetLockupCheckedInstructionData> {
  return getStructDecoder([
    ['discriminator', getU32Decoder()],
    ['unixTimestamp', getOptionDecoder(getI64Decoder())],
    ['epoch', getOptionDecoder(getU64Decoder())],
  ]);
}

export function getSetLockupCheckedInstructionDataCodec(): Codec<
  SetLockupCheckedInstructionDataArgs,
  SetLockupCheckedInstructionData
> {
  return combineCodec(
    getSetLockupCheckedInstructionDataEncoder(),
    getSetLockupCheckedInstructionDataDecoder()
  );
}

export type SetLockupCheckedInput<
  TAccountStake extends string = string,
  TAccountAuthority extends string = string,
  TAccountNewAuthority extends string = string,
> = {
  /** Initialized stake account */
  stake: Address<TAccountStake>;
  /** Lockup authority or withdraw authority */
  authority: TransactionSigner<TAccountAuthority>;
  /** New lockup authority */
  newAuthority?: TransactionSigner<TAccountNewAuthority>;
  unixTimestamp: SetLockupCheckedInstructionDataArgs['unixTimestamp'];
  epoch: SetLockupCheckedInstructionDataArgs['epoch'];
};

export function getSetLockupCheckedInstruction<
  TAccountStake extends string,
  TAccountAuthority extends string,
  TAccountNewAuthority extends string,
  TProgramAddress extends Address = typeof STAKE_PROGRAM_ADDRESS,
>(
  input: SetLockupCheckedInput<
    TAccountStake,
    TAccountAuthority,
    TAccountNewAuthority
  >,
  config?: { programAddress?: TProgramAddress }
): SetLockupCheckedInstruction<
  TProgramAddress,
  TAccountStake,
  TAccountAuthority,
  TAccountNewAuthority
> {
  // Program address.
  const programAddress = config?.programAddress ?? STAKE_PROGRAM_ADDRESS;

  // Original accounts.
  const originalAccounts = {
    stake: { value: input.stake ?? null, isWritable: true },
    authority: { value: input.authority ?? null, isWritable: false },
    newAuthority: { value: input.newAuthority ?? null, isWritable: false },
  };
  const accounts = originalAccounts as Record<
    keyof typeof originalAccounts,
    ResolvedAccount
  >;

  // Original args.
  const args = { ...input };

  const getAccountMeta = getAccountMetaFactory(programAddress, 'programId');
  const instruction = {
    accounts: [
      getAccountMeta(accounts.stake),
      getAccountMeta(accounts.authority),
      getAccountMeta(accounts.newAuthority),
    ],
    programAddress,
    data: getSetLockupCheckedInstructionDataEncoder().encode(
      args as SetLockupCheckedInstructionDataArgs
    ),
  } as SetLockupCheckedInstruction<
    TProgramAddress,
    TAccountStake,
    TAccountAuthority,
    TAccountNewAuthority
  >;

  return instruction;
}

export type ParsedSetLockupCheckedInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountMetas extends readonly IAccountMeta[] = readonly IAccountMeta[],
> = {
  programAddress: Address<TProgram>;
  accounts: {
    /** Initialized stake account */
    stake: TAccountMetas[0];
    /** Lockup authority or withdraw authority */
    authority: TAccountMetas[1];
    /** New lockup authority */
    newAuthority?: TAccountMetas[2] | undefined;
  };
  data: SetLockupCheckedInstructionData;
};

export function parseSetLockupCheckedInstruction<
  TProgram extends string,
  TAccountMetas extends readonly IAccountMeta[],
>(
  instruction: IInstruction<TProgram> &
    IInstructionWithAccounts<TAccountMetas> &
    IInstructionWithData<Uint8Array>
): ParsedSetLockupCheckedInstruction<TProgram, TAccountMetas> {
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
      authority: getNextAccount(),
      newAuthority: getNextOptionalAccount(),
    },
    data: getSetLockupCheckedInstructionDataDecoder().decode(instruction.data),
  };
}
