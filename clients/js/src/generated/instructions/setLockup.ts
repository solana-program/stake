/**
 * This code was AUTOGENERATED using the codama library.
 * Please DO NOT EDIT THIS FILE, instead use visitors
 * to add features, then rerun codama to update it.
 *
 * @see https://github.com/codama-idl/codama
 */

import {
  combineCodec,
  getAddressDecoder,
  getAddressEncoder,
  getI64Decoder,
  getI64Encoder,
  getOptionDecoder,
  getOptionEncoder,
  getStructDecoder,
  getStructEncoder,
  getU64Decoder,
  getU64Encoder,
  getU8Decoder,
  getU8Encoder,
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

export const SET_LOCKUP_DISCRIMINATOR = 6;

export function getSetLockupDiscriminatorBytes() {
  return getU8Encoder().encode(SET_LOCKUP_DISCRIMINATOR);
}

export type SetLockupInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountStake extends string | IAccountMeta<string> = string,
  TAccountAuthority extends string | IAccountMeta<string> = string,
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
      ...TRemainingAccounts,
    ]
  >;

export type SetLockupInstructionData = {
  discriminator: number;
  unixTimestamp: Option<bigint>;
  epoch: Option<bigint>;
  custodian: Option<Address>;
};

export type SetLockupInstructionDataArgs = {
  unixTimestamp: OptionOrNullable<number | bigint>;
  epoch: OptionOrNullable<number | bigint>;
  custodian: OptionOrNullable<Address>;
};

export function getSetLockupInstructionDataEncoder(): Encoder<SetLockupInstructionDataArgs> {
  return transformEncoder(
    getStructEncoder([
      ['discriminator', getU8Encoder()],
      ['unixTimestamp', getOptionEncoder(getI64Encoder())],
      ['epoch', getOptionEncoder(getU64Encoder())],
      ['custodian', getOptionEncoder(getAddressEncoder())],
    ]),
    (value) => ({ ...value, discriminator: SET_LOCKUP_DISCRIMINATOR })
  );
}

export function getSetLockupInstructionDataDecoder(): Decoder<SetLockupInstructionData> {
  return getStructDecoder([
    ['discriminator', getU8Decoder()],
    ['unixTimestamp', getOptionDecoder(getI64Decoder())],
    ['epoch', getOptionDecoder(getU64Decoder())],
    ['custodian', getOptionDecoder(getAddressDecoder())],
  ]);
}

export function getSetLockupInstructionDataCodec(): Codec<
  SetLockupInstructionDataArgs,
  SetLockupInstructionData
> {
  return combineCodec(
    getSetLockupInstructionDataEncoder(),
    getSetLockupInstructionDataDecoder()
  );
}

export type SetLockupInput<
  TAccountStake extends string = string,
  TAccountAuthority extends string = string,
> = {
  /** Initialized stake account */
  stake: Address<TAccountStake>;
  /** Lockup authority or withdraw authority */
  authority: TransactionSigner<TAccountAuthority>;
  unixTimestamp: SetLockupInstructionDataArgs['unixTimestamp'];
  epoch: SetLockupInstructionDataArgs['epoch'];
  custodian: SetLockupInstructionDataArgs['custodian'];
};

export function getSetLockupInstruction<
  TAccountStake extends string,
  TAccountAuthority extends string,
  TProgramAddress extends Address = typeof STAKE_PROGRAM_ADDRESS,
>(
  input: SetLockupInput<TAccountStake, TAccountAuthority>,
  config?: { programAddress?: TProgramAddress }
): SetLockupInstruction<TProgramAddress, TAccountStake, TAccountAuthority> {
  // Program address.
  const programAddress = config?.programAddress ?? STAKE_PROGRAM_ADDRESS;

  // Original accounts.
  const originalAccounts = {
    stake: { value: input.stake ?? null, isWritable: true },
    authority: { value: input.authority ?? null, isWritable: false },
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
    ],
    programAddress,
    data: getSetLockupInstructionDataEncoder().encode(
      args as SetLockupInstructionDataArgs
    ),
  } as SetLockupInstruction<TProgramAddress, TAccountStake, TAccountAuthority>;

  return instruction;
}

export type ParsedSetLockupInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountMetas extends readonly IAccountMeta[] = readonly IAccountMeta[],
> = {
  programAddress: Address<TProgram>;
  accounts: {
    /** Initialized stake account */
    stake: TAccountMetas[0];
    /** Lockup authority or withdraw authority */
    authority: TAccountMetas[1];
  };
  data: SetLockupInstructionData;
};

export function parseSetLockupInstruction<
  TProgram extends string,
  TAccountMetas extends readonly IAccountMeta[],
>(
  instruction: IInstruction<TProgram> &
    IInstructionWithAccounts<TAccountMetas> &
    IInstructionWithData<Uint8Array>
): ParsedSetLockupInstruction<TProgram, TAccountMetas> {
  if (instruction.accounts.length < 2) {
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
      authority: getNextAccount(),
    },
    data: getSetLockupInstructionDataDecoder().decode(instruction.data),
  };
}
