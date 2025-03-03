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
  type ReadonlyAccount,
  type ReadonlySignerAccount,
  type TransactionSigner,
  type WritableAccount,
} from '@solana/kit';
import { STAKE_PROGRAM_ADDRESS } from '../programs';
import { getAccountMetaFactory, type ResolvedAccount } from '../shared';

export const WITHDRAW_DISCRIMINATOR = 4;

export function getWithdrawDiscriminatorBytes() {
  return getU32Encoder().encode(WITHDRAW_DISCRIMINATOR);
}

export type WithdrawInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountStake extends string | IAccountMeta<string> = string,
  TAccountRecipient extends string | IAccountMeta<string> = string,
  TAccountClockSysvar extends
    | string
    | IAccountMeta<string> = 'SysvarC1ock11111111111111111111111111111111',
  TAccountStakeHistory extends string | IAccountMeta<string> = string,
  TAccountWithdrawAuthority extends string | IAccountMeta<string> = string,
  TAccountLockupAuthority extends string | IAccountMeta<string> = string,
  TRemainingAccounts extends readonly IAccountMeta<string>[] = [],
> = IInstruction<TProgram> &
  IInstructionWithData<Uint8Array> &
  IInstructionWithAccounts<
    [
      TAccountStake extends string
        ? WritableAccount<TAccountStake>
        : TAccountStake,
      TAccountRecipient extends string
        ? WritableAccount<TAccountRecipient>
        : TAccountRecipient,
      TAccountClockSysvar extends string
        ? ReadonlyAccount<TAccountClockSysvar>
        : TAccountClockSysvar,
      TAccountStakeHistory extends string
        ? ReadonlyAccount<TAccountStakeHistory>
        : TAccountStakeHistory,
      TAccountWithdrawAuthority extends string
        ? ReadonlySignerAccount<TAccountWithdrawAuthority> &
            IAccountSignerMeta<TAccountWithdrawAuthority>
        : TAccountWithdrawAuthority,
      TAccountLockupAuthority extends string
        ? ReadonlySignerAccount<TAccountLockupAuthority> &
            IAccountSignerMeta<TAccountLockupAuthority>
        : TAccountLockupAuthority,
      ...TRemainingAccounts,
    ]
  >;

export type WithdrawInstructionData = { discriminator: number; args: bigint };

export type WithdrawInstructionDataArgs = { args: number | bigint };

export function getWithdrawInstructionDataEncoder(): Encoder<WithdrawInstructionDataArgs> {
  return transformEncoder(
    getStructEncoder([
      ['discriminator', getU32Encoder()],
      ['args', getU64Encoder()],
    ]),
    (value) => ({ ...value, discriminator: WITHDRAW_DISCRIMINATOR })
  );
}

export function getWithdrawInstructionDataDecoder(): Decoder<WithdrawInstructionData> {
  return getStructDecoder([
    ['discriminator', getU32Decoder()],
    ['args', getU64Decoder()],
  ]);
}

export function getWithdrawInstructionDataCodec(): Codec<
  WithdrawInstructionDataArgs,
  WithdrawInstructionData
> {
  return combineCodec(
    getWithdrawInstructionDataEncoder(),
    getWithdrawInstructionDataDecoder()
  );
}

export type WithdrawInput<
  TAccountStake extends string = string,
  TAccountRecipient extends string = string,
  TAccountClockSysvar extends string = string,
  TAccountStakeHistory extends string = string,
  TAccountWithdrawAuthority extends string = string,
  TAccountLockupAuthority extends string = string,
> = {
  /** Stake account from which to withdraw */
  stake: Address<TAccountStake>;
  /** Recipient account */
  recipient: Address<TAccountRecipient>;
  /** Clock sysvar */
  clockSysvar?: Address<TAccountClockSysvar>;
  /** Stake history sysvar */
  stakeHistory: Address<TAccountStakeHistory>;
  /** Withdraw authority */
  withdrawAuthority: TransactionSigner<TAccountWithdrawAuthority>;
  /** Lockup authority */
  lockupAuthority?: TransactionSigner<TAccountLockupAuthority>;
  args: WithdrawInstructionDataArgs['args'];
};

export function getWithdrawInstruction<
  TAccountStake extends string,
  TAccountRecipient extends string,
  TAccountClockSysvar extends string,
  TAccountStakeHistory extends string,
  TAccountWithdrawAuthority extends string,
  TAccountLockupAuthority extends string,
  TProgramAddress extends Address = typeof STAKE_PROGRAM_ADDRESS,
>(
  input: WithdrawInput<
    TAccountStake,
    TAccountRecipient,
    TAccountClockSysvar,
    TAccountStakeHistory,
    TAccountWithdrawAuthority,
    TAccountLockupAuthority
  >,
  config?: { programAddress?: TProgramAddress }
): WithdrawInstruction<
  TProgramAddress,
  TAccountStake,
  TAccountRecipient,
  TAccountClockSysvar,
  TAccountStakeHistory,
  TAccountWithdrawAuthority,
  TAccountLockupAuthority
> {
  // Program address.
  const programAddress = config?.programAddress ?? STAKE_PROGRAM_ADDRESS;

  // Original accounts.
  const originalAccounts = {
    stake: { value: input.stake ?? null, isWritable: true },
    recipient: { value: input.recipient ?? null, isWritable: true },
    clockSysvar: { value: input.clockSysvar ?? null, isWritable: false },
    stakeHistory: { value: input.stakeHistory ?? null, isWritable: false },
    withdrawAuthority: {
      value: input.withdrawAuthority ?? null,
      isWritable: false,
    },
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
      getAccountMeta(accounts.recipient),
      getAccountMeta(accounts.clockSysvar),
      getAccountMeta(accounts.stakeHistory),
      getAccountMeta(accounts.withdrawAuthority),
      getAccountMeta(accounts.lockupAuthority),
    ],
    programAddress,
    data: getWithdrawInstructionDataEncoder().encode(
      args as WithdrawInstructionDataArgs
    ),
  } as WithdrawInstruction<
    TProgramAddress,
    TAccountStake,
    TAccountRecipient,
    TAccountClockSysvar,
    TAccountStakeHistory,
    TAccountWithdrawAuthority,
    TAccountLockupAuthority
  >;

  return instruction;
}

export type ParsedWithdrawInstruction<
  TProgram extends string = typeof STAKE_PROGRAM_ADDRESS,
  TAccountMetas extends readonly IAccountMeta[] = readonly IAccountMeta[],
> = {
  programAddress: Address<TProgram>;
  accounts: {
    /** Stake account from which to withdraw */
    stake: TAccountMetas[0];
    /** Recipient account */
    recipient: TAccountMetas[1];
    /** Clock sysvar */
    clockSysvar: TAccountMetas[2];
    /** Stake history sysvar */
    stakeHistory: TAccountMetas[3];
    /** Withdraw authority */
    withdrawAuthority: TAccountMetas[4];
    /** Lockup authority */
    lockupAuthority?: TAccountMetas[5] | undefined;
  };
  data: WithdrawInstructionData;
};

export function parseWithdrawInstruction<
  TProgram extends string,
  TAccountMetas extends readonly IAccountMeta[],
>(
  instruction: IInstruction<TProgram> &
    IInstructionWithAccounts<TAccountMetas> &
    IInstructionWithData<Uint8Array>
): ParsedWithdrawInstruction<TProgram, TAccountMetas> {
  if (instruction.accounts.length < 6) {
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
      recipient: getNextAccount(),
      clockSysvar: getNextAccount(),
      stakeHistory: getNextAccount(),
      withdrawAuthority: getNextAccount(),
      lockupAuthority: getNextOptionalAccount(),
    },
    data: getWithdrawInstructionDataDecoder().decode(instruction.data),
  };
}
