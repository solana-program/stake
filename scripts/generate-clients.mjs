#!/usr/bin/env zx
import 'zx/globals';
import * as c from 'codama';
import { rootNodeFromAnchor } from '@codama/nodes-from-anchor';
import { renderVisitor as renderJavaScriptVisitor } from '@codama/renderers-js';
import { renderVisitor as renderRustVisitor } from '@codama/renderers-rust';
import { getToolchainArgument } from './utils.mjs';

// Instanciate Codama from the IDL.
const idl = require(path.join(__dirname, '..', 'interface', 'idl.json'));
const codama = c.createFromRoot(rootNodeFromAnchor(idl));

// Rename the program.
codama.update(
  c.updateProgramsVisitor({
    solanaStakeInterface: { name: 'stake' },
  })
);

codama.update(
  c.updateInstructionsVisitor({
    // Deprecated instruction.
    redelegate: { delete: true },
  })
);

// Add missing types from the IDL.
codama.update(
  c.bottomUpTransformerVisitor([
    {
      select: '[programNode]stake',
      transform: (node) => {
        c.assertIsNode(node, 'programNode');
        return {
          ...node,
          errors: [
            {
              code: 0,
              name: 'NoCreditsToRedeem',
              message: 'Not enough credits to redeem',
            },
            {
              code: 1,
              name: 'LockupInForce',
              message: 'Lockup has not yet expired',
            },
            {
              code: 2,
              name: 'AlreadyDeactivated',
              message: 'Stake already deactivated',
            },
            {
              code: 3,
              name: 'TooSoonToRedelegate',
              message: 'One re-delegation permitted per epoch',
            },
            {
              code: 4,
              name: 'InsufficientStake',
              message: 'Split amount is more than is staked',
            },
            {
              code: 5,
              name: 'MergeTransientStake',
              message: 'Stake account with transient stake cannot be merged',
            },
            {
              code: 6,
              name: 'MergeMismatch',
              message:
                'Stake account merge failed due to different authority, lockups or state',
            },
            {
              code: 7,
              name: 'CustodianMissing',
              message: 'Custodian address not present',
            },
            {
              code: 8,
              name: 'CustodianSignatureMissing',
              message: 'Custodian signature not present',
            },
            {
              code: 9,
              name: 'InsufficientReferenceVotes',
              message:
                'Insufficient voting activity in the reference vote account',
            },
            {
              code: 10,
              name: 'VoteAddressMismatch',
              message:
                'Stake account is not delegated to the provided vote account',
            },
            {
              code: 11,
              name: 'MinimumDelinquentEpochsForDeactivationNotMet',
              message:
                'Stake account has not been delinquent for the minimum epochs required for deactivation',
            },
            {
              code: 12,
              name: 'InsufficientDelegation',
              message: 'Delegation amount is less than the minimum',
            },
            {
              code: 13,
              name: 'RedelegateTransientOrInactiveStake',
              message:
                'Stake account with transient or inactive stake cannot be redelegated',
            },
            {
              code: 14,
              name: 'RedelegateToSameVoteAccount',
              message:
                'Stake redelegation to the same vote account is not permitted',
            },
            {
              code: 15,
              name: 'RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted',
              message:
                'Redelegated stake must be fully activated before deactivation',
            },
            {
              code: 16,
              name: 'EpochRewardsActive',
              message:
                'Stake action is not permitted while the epoch rewards period is active',
            },
          ],
        };
      },
    },
    {
      // Epoch -> u64
      select: '[definedTypeLinkNode]epoch',
      transform: () => {
        return c.numberTypeNode('u64');
      },
    },
    {
      // UnixTimestamp -> i64
      select: '[definedTypeLinkNode]unixTimestamp',
      transform: () => {
        return c.numberTypeNode('i64');
      },
    },
    {
      // f64 -> f64
      select: '[definedTypeLinkNode]f64',
      transform: () => {
        return c.numberTypeNode('f64');
      },
    },
  ])
);

// Render JavaScript.
const jsClient = path.join(__dirname, '..', 'clients', 'js');
codama.accept(
  renderJavaScriptVisitor(path.join(jsClient, 'src', 'generated'), {
    prettierOptions: require(path.join(jsClient, '.prettierrc.json')),
  })
);

// Render Rust.
const rustClient = path.join(__dirname, '..', 'clients', 'rust');
codama.accept(
  renderRustVisitor(path.join(rustClient, 'src', 'generated'), {
    formatCode: true,
    crateFolder: rustClient,
    anchorTraits: false,
    toolchain: getToolchainArgument('format'),
    traitOptions: {
      overrides: {
        delegation: ['borsh::BorshSerialize', 'borsh::BorshDeserialize', 'Clone', 'Debug'],
        stake: ['borsh::BorshSerialize', 'borsh::BorshDeserialize', 'Clone', 'Debug'],
        stakeState: ['borsh::BorshSerialize', 'borsh::BorshDeserialize', 'Clone', 'Debug'],
        stakeStateV2: ['borsh::BorshSerialize', 'borsh::BorshDeserialize', 'Clone', 'Debug'],
      },
    },
  })
);
