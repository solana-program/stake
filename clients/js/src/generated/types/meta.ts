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
  type Codec,
  type Decoder,
  type Encoder,
} from '@solana/kit';
import {
  getAuthorizedDecoder,
  getAuthorizedEncoder,
  getLockupDecoder,
  getLockupEncoder,
  type Authorized,
  type AuthorizedArgs,
  type Lockup,
  type LockupArgs,
} from '.';

export type Meta = {
  rentExemptReserve: bigint;
  authorized: Authorized;
  lockup: Lockup;
};

export type MetaArgs = {
  rentExemptReserve: number | bigint;
  authorized: AuthorizedArgs;
  lockup: LockupArgs;
};

export function getMetaEncoder(): Encoder<MetaArgs> {
  return getStructEncoder([
    ['rentExemptReserve', getU64Encoder()],
    ['authorized', getAuthorizedEncoder()],
    ['lockup', getLockupEncoder()],
  ]);
}

export function getMetaDecoder(): Decoder<Meta> {
  return getStructDecoder([
    ['rentExemptReserve', getU64Decoder()],
    ['authorized', getAuthorizedDecoder()],
    ['lockup', getLockupDecoder()],
  ]);
}

export function getMetaCodec(): Codec<MetaArgs, Meta> {
  return combineCodec(getMetaEncoder(), getMetaDecoder());
}
