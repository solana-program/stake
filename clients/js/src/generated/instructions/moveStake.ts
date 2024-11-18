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
  type ReadonlySignerAccount,
  type TransactionSigner,
  type WritableAccount,
} from '@solana/web3.js';
import { STAKE_PROGRAM_ADDRESS } from '../programs';
import { getAccountMetaFactory, type ResolvedAccount } from '../shared';

export const MOVE_STAKE_DISCRIMINATOR = 16;

export function getMoveStakeDiscriminatorBytes() {
  return getU8Encoder().encode(MOVE_STAKE_DISCRIMINATOR);
}

export type MoveStakeInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountSourceStake extends string | IAccountMeta<string> = string,
  TAccountDestinationStake extends string | IAccountMeta<string> = string,
  TAccountStakeAuthority extends string | IAccountMeta<string> = string,
  TRemainingAccounts extends readonly IAccountMeta<string>[] = [],
> = IInstruction<TProgram> &
  IInstructionWithData<Uint8Array> &
  IInstructionWithAccounts<
    [
      TAccountSourceStake extends string
        ? WritableAccount<TAccountSourceStake>
        : TAccountSourceStake,
      TAccountDestinationStake extends string
        ? WritableAccount<TAccountDestinationStake>
        : TAccountDestinationStake,
      TAccountStakeAuthority extends string
        ? ReadonlySignerAccount<TAccountStakeAuthority> &
            IAccountSignerMeta<TAccountStakeAuthority>
        : TAccountStakeAuthority,
      ...TRemainingAccounts,
    ]
  >;

export type MoveStakeInstructionData = { discriminator: number; args: bigint };

export type MoveStakeInstructionDataArgs = { args: number | bigint };

export function getMoveStakeInstructionDataEncoder(): Encoder<MoveStakeInstructionDataArgs> {
  return transformEncoder(
    getStructEncoder([
      ['discriminator', getU8Encoder()],
      ['args', getU64Encoder()],
    ]),
    (value) => ({ ...value, discriminator: MOVE_STAKE_DISCRIMINATOR })
  );
}

export function getMoveStakeInstructionDataDecoder(): Decoder<MoveStakeInstructionData> {
  return getStructDecoder([
    ['discriminator', getU8Decoder()],
    ['args', getU64Decoder()],
  ]);
}

export function getMoveStakeInstructionDataCodec(): Codec<
  MoveStakeInstructionDataArgs,
  MoveStakeInstructionData
> {
  return combineCodec(
    getMoveStakeInstructionDataEncoder(),
    getMoveStakeInstructionDataDecoder()
  );
}

export type MoveStakeInput<
  TAccountSourceStake extends string = string,
  TAccountDestinationStake extends string = string,
  TAccountStakeAuthority extends string = string,
> = {
  /** Active source stake account */
  sourceStake: Address<TAccountSourceStake>;
  /** Active or inactive destination stake account */
  destinationStake: Address<TAccountDestinationStake>;
  /** Stake authority */
  stakeAuthority: TransactionSigner<TAccountStakeAuthority>;
  args: MoveStakeInstructionDataArgs['args'];
};

export function getMoveStakeInstruction<
  TAccountSourceStake extends string,
  TAccountDestinationStake extends string,
  TAccountStakeAuthority extends string,
  TProgramAddress extends Address = typeof STAKE_PROGRAM_ADDRESS,
>(
  input: MoveStakeInput<
    TAccountSourceStake,
    TAccountDestinationStake,
    TAccountStakeAuthority
  >,
  config?: { programAddress?: TProgramAddress }
): MoveStakeInstruction<
  TProgramAddress,
  TAccountSourceStake,
  TAccountDestinationStake,
  TAccountStakeAuthority
> {
  // Program address.
  const programAddress = config?.programAddress ?? STAKE_PROGRAM_ADDRESS;

  // Original accounts.
  const originalAccounts = {
    sourceStake: { value: input.sourceStake ?? null, isWritable: true },
    destinationStake: {
      value: input.destinationStake ?? null,
      isWritable: true,
    },
    stakeAuthority: { value: input.stakeAuthority ?? null, isWritable: false },
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
      getAccountMeta(accounts.sourceStake),
      getAccountMeta(accounts.destinationStake),
      getAccountMeta(accounts.stakeAuthority),
    ],
    programAddress,
    data: getMoveStakeInstructionDataEncoder().encode(
      args as MoveStakeInstructionDataArgs
    ),
  } as MoveStakeInstruction<
    TProgramAddress,
    TAccountSourceStake,
    TAccountDestinationStake,
    TAccountStakeAuthority
  >;

  return instruction;
}

export type ParsedMoveStakeInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountMetas extends readonly IAccountMeta[] = readonly IAccountMeta[],
> = {
  programAddress: Address<TProgram>;
  accounts: {
    /** Active source stake account */
    sourceStake: TAccountMetas[0];
    /** Active or inactive destination stake account */
    destinationStake: TAccountMetas[1];
    /** Stake authority */
    stakeAuthority: TAccountMetas[2];
  };
  data: MoveStakeInstructionData;
};

export function parseMoveStakeInstruction<
  TProgram extends string,
  TAccountMetas extends readonly IAccountMeta[],
>(
  instruction: IInstruction<TProgram> &
    IInstructionWithAccounts<TAccountMetas> &
    IInstructionWithData<Uint8Array>
): ParsedMoveStakeInstruction<TProgram, TAccountMetas> {
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
      sourceStake: getNextAccount(),
      destinationStake: getNextAccount(),
      stakeAuthority: getNextAccount(),
    },
    data: getMoveStakeInstructionDataDecoder().decode(instruction.data),
  };
}
