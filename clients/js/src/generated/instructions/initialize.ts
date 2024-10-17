/**
 * This code was AUTOGENERATED using the codama library.
 * Please DO NOT EDIT THIS FILE, instead use visitors
 * to add features, then rerun codama to update it.
 *
 * @see https://github.com/codama-idl/codama
 */

import {
  combineCodec,
  fixDecoderSize,
  fixEncoderSize,
  getAddressDecoder,
  getAddressEncoder,
  getBytesDecoder,
  getBytesEncoder,
  getI64Decoder,
  getI64Encoder,
  getStructDecoder,
  getStructEncoder,
  getU64Decoder,
  getU64Encoder,
  transformEncoder,
  type Address,
  type Codec,
  type Decoder,
  type Encoder,
  type IAccountMeta,
  type IInstruction,
  type IInstructionWithAccounts,
  type IInstructionWithData,
  type ReadonlyAccount,
  type ReadonlyUint8Array,
  type WritableAccount,
} from '@solana/web3.js';
import { STAKE_PROGRAM_PROGRAM_ADDRESS } from '../programs';
import { getAccountMetaFactory, type ResolvedAccount } from '../shared';

export const INITIALIZE_DISCRIMINATOR = new Uint8Array([
  175, 175, 109, 31, 13, 152, 155, 237,
]);

export function getInitializeDiscriminatorBytes() {
  return fixEncoderSize(getBytesEncoder(), 8).encode(INITIALIZE_DISCRIMINATOR);
}

export type InitializeInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_PROGRAM_ADDRESS,
  TAccountStake extends string | IAccountMeta<string> = string,
  TAccountRent extends
    | string
    | IAccountMeta<string> = 'SysvarRent111111111111111111111111111111111',
  TRemainingAccounts extends readonly IAccountMeta<string>[] = [],
> = IInstruction<TProgram> &
  IInstructionWithData<Uint8Array> &
  IInstructionWithAccounts<
    [
      TAccountStake extends string
        ? WritableAccount<TAccountStake>
        : TAccountStake,
      TAccountRent extends string
        ? ReadonlyAccount<TAccountRent>
        : TAccountRent,
      ...TRemainingAccounts,
    ]
  >;

export type InitializeInstructionData = {
  discriminator: ReadonlyUint8Array;
  staker: Address;
  withdrawer: Address;
  unixTimestamp: bigint;
  epoch: bigint;
  custodian: Address;
};

export type InitializeInstructionDataArgs = {
  staker: Address;
  withdrawer: Address;
  unixTimestamp: number | bigint;
  epoch: number | bigint;
  custodian: Address;
};

export function getInitializeInstructionDataEncoder(): Encoder<InitializeInstructionDataArgs> {
  return transformEncoder(
    getStructEncoder([
      ['discriminator', fixEncoderSize(getBytesEncoder(), 8)],
      ['staker', getAddressEncoder()],
      ['withdrawer', getAddressEncoder()],
      ['unixTimestamp', getI64Encoder()],
      ['epoch', getU64Encoder()],
      ['custodian', getAddressEncoder()],
    ]),
    (value) => ({ ...value, discriminator: INITIALIZE_DISCRIMINATOR })
  );
}

export function getInitializeInstructionDataDecoder(): Decoder<InitializeInstructionData> {
  return getStructDecoder([
    ['discriminator', fixDecoderSize(getBytesDecoder(), 8)],
    ['staker', getAddressDecoder()],
    ['withdrawer', getAddressDecoder()],
    ['unixTimestamp', getI64Decoder()],
    ['epoch', getU64Decoder()],
    ['custodian', getAddressDecoder()],
  ]);
}

export function getInitializeInstructionDataCodec(): Codec<
  InitializeInstructionDataArgs,
  InitializeInstructionData
> {
  return combineCodec(
    getInitializeInstructionDataEncoder(),
    getInitializeInstructionDataDecoder()
  );
}

export type InitializeInput<
  TAccountStake extends string = string,
  TAccountRent extends string = string,
> = {
  /** The stake account to initialize */
  stake: Address<TAccountStake>;
  /** Rent sysvar */
  rent?: Address<TAccountRent>;
  staker: InitializeInstructionDataArgs['staker'];
  withdrawer: InitializeInstructionDataArgs['withdrawer'];
  unixTimestamp: InitializeInstructionDataArgs['unixTimestamp'];
  epoch: InitializeInstructionDataArgs['epoch'];
  custodian: InitializeInstructionDataArgs['custodian'];
};

export function getInitializeInstruction<
  TAccountStake extends string,
  TAccountRent extends string,
  TProgramAddress extends Address = typeof STAKE_PROGRAM_PROGRAM_ADDRESS,
>(
  input: InitializeInput<TAccountStake, TAccountRent>,
  config?: { programAddress?: TProgramAddress }
): InitializeInstruction<TProgramAddress, TAccountStake, TAccountRent> {
  // Program address.
  const programAddress =
    config?.programAddress ?? STAKE_PROGRAM_PROGRAM_ADDRESS;

  // Original accounts.
  const originalAccounts = {
    stake: { value: input.stake ?? null, isWritable: true },
    rent: { value: input.rent ?? null, isWritable: false },
  };
  const accounts = originalAccounts as Record<
    keyof typeof originalAccounts,
    ResolvedAccount
  >;

  // Original args.
  const args = { ...input };

  // Resolve default values.
  if (!accounts.rent.value) {
    accounts.rent.value =
      'SysvarRent111111111111111111111111111111111' as Address<'SysvarRent111111111111111111111111111111111'>;
  }

  const getAccountMeta = getAccountMetaFactory(programAddress, 'programId');
  const instruction = {
    accounts: [getAccountMeta(accounts.stake), getAccountMeta(accounts.rent)],
    programAddress,
    data: getInitializeInstructionDataEncoder().encode(
      args as InitializeInstructionDataArgs
    ),
  } as InitializeInstruction<TProgramAddress, TAccountStake, TAccountRent>;

  return instruction;
}

export type ParsedInitializeInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_PROGRAM_ADDRESS,
  TAccountMetas extends readonly IAccountMeta[] = readonly IAccountMeta[],
> = {
  programAddress: Address<TProgram>;
  accounts: {
    /** The stake account to initialize */
    stake: TAccountMetas[0];
    /** Rent sysvar */
    rent: TAccountMetas[1];
  };
  data: InitializeInstructionData;
};

export function parseInitializeInstruction<
  TProgram extends string,
  TAccountMetas extends readonly IAccountMeta[],
>(
  instruction: IInstruction<TProgram> &
    IInstructionWithAccounts<TAccountMetas> &
    IInstructionWithData<Uint8Array>
): ParsedInitializeInstruction<TProgram, TAccountMetas> {
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
      rent: getNextAccount(),
    },
    data: getInitializeInstructionDataDecoder().decode(instruction.data),
  };
}