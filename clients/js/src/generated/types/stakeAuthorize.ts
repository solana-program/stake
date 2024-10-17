/**
 * This code was AUTOGENERATED using the codama library.
 * Please DO NOT EDIT THIS FILE, instead use visitors
 * to add features, then rerun codama to update it.
 *
 * @see https://github.com/codama-idl/codama
 */

import {
  combineCodec,
  getEnumDecoder,
  getEnumEncoder,
  type Codec,
  type Decoder,
  type Encoder,
} from '@solana/web3.js';

export enum StakeAuthorize {
  Staker,
  Withdrawer,
}

export type StakeAuthorizeArgs = StakeAuthorize;

export function getStakeAuthorizeEncoder(): Encoder<StakeAuthorizeArgs> {
  return getEnumEncoder(StakeAuthorize);
}

export function getStakeAuthorizeDecoder(): Decoder<StakeAuthorize> {
  return getEnumDecoder(StakeAuthorize);
}

export function getStakeAuthorizeCodec(): Codec<
  StakeAuthorizeArgs,
  StakeAuthorize
> {
  return combineCodec(getStakeAuthorizeEncoder(), getStakeAuthorizeDecoder());
}
