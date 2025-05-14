#!/usr/bin/env zx
import 'zx/globals';
import * as c from 'codama';
import { rootNodeFromAnchor } from '@codama/nodes-from-anchor';
import { renderVisitor as renderJavaScriptVisitor } from '@codama/renderers-js';
import { renderVisitor as renderRustVisitor } from '@codama/renderers-rust';
import { getToolchainArgument, workingDirectory } from './utils.mjs';

// Instanciate Codama from the IDL.
const idl = JSON.parse(
  fs.readFileSync(path.join(workingDirectory, 'interface', 'idl.json'), 'utf-8')
);
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
          accounts: [
            ...node.accounts,
            // stake account
            c.accountNode({
              name: 'stakeStateAccount',
              data: c.structTypeNode([
                c.structFieldTypeNode({
                  name: 'state',
                  type: c.definedTypeLinkNode('stakeStateV2'),
                }),
              ]),
            }),
          ],
          errors: [
            c.errorNode({
              code: 0,
              name: 'NoCreditsToRedeem',
              message: 'Not enough credits to redeem',
            }),
            c.errorNode({
              code: 1,
              name: 'LockupInForce',
              message: 'Lockup has not yet expired',
            }),
            c.errorNode({
              code: 2,
              name: 'AlreadyDeactivated',
              message: 'Stake already deactivated',
            }),
            c.errorNode({
              code: 3,
              name: 'TooSoonToRedelegate',
              message: 'One re-delegation permitted per epoch',
            }),
            c.errorNode({
              code: 4,
              name: 'InsufficientStake',
              message: 'Split amount is more than is staked',
            }),
            c.errorNode({
              code: 5,
              name: 'MergeTransientStake',
              message: 'Stake account with transient stake cannot be merged',
            }),
            c.errorNode({
              code: 6,
              name: 'MergeMismatch',
              message:
                'Stake account merge failed due to different authority, lockups or state',
            }),
            c.errorNode({
              code: 7,
              name: 'CustodianMissing',
              message: 'Custodian address not present',
            }),
            c.errorNode({
              code: 8,
              name: 'CustodianSignatureMissing',
              message: 'Custodian signature not present',
            }),
            c.errorNode({
              code: 9,
              name: 'InsufficientReferenceVotes',
              message:
                'Insufficient voting activity in the reference vote account',
            }),
            c.errorNode({
              code: 10,
              name: 'VoteAddressMismatch',
              message:
                'Stake account is not delegated to the provided vote account',
            }),
            c.errorNode({
              code: 11,
              name: 'MinimumDelinquentEpochsForDeactivationNotMet',
              message:
                'Stake account has not been delinquent for the minimum epochs required for deactivation',
            }),
            c.errorNode({
              code: 12,
              name: 'InsufficientDelegation',
              message: 'Delegation amount is less than the minimum',
            }),
            c.errorNode({
              code: 13,
              name: 'RedelegateTransientOrInactiveStake',
              message:
                'Stake account with transient or inactive stake cannot be redelegated',
            }),
            c.errorNode({
              code: 14,
              name: 'RedelegateToSameVoteAccount',
              message:
                'Stake redelegation to the same vote account is not permitted',
            }),
            c.errorNode({
              code: 15,
              name: 'RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted',
              message:
                'Redelegated stake must be fully activated before deactivation',
            }),
            c.errorNode({
              code: 16,
              name: 'EpochRewardsActive',
              message:
                'Stake action is not permitted while the epoch rewards period is active',
            }),
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
      // [definedType]f64 -> [numberType]f64
      select: '[definedTypeLinkNode]f64',
      transform: () => {
        return c.numberTypeNode('f64');
      },
    },
    {
      // enum discriminator -> u32
      select: '[definedTypeNode]stakeState.[enumTypeNode]',
      transform: (node) => {
        c.assertIsNode(node, 'enumTypeNode');
        return {
          ...node,
          size: c.numberTypeNode('u32'),
        };
      },
    },
    {
      // enum discriminator -> u32
      select: '[definedTypeNode]stakeStateV2.[enumTypeNode]',
      transform: (node) => {
        c.assertIsNode(node, 'enumTypeNode');
        return {
          ...node,
          size: c.numberTypeNode('u32'),
        };
      },
    },
    {
      // Use omitted optional account strategy for all instructions.
      select: '[instructionNode]',
      transform: (node) => {
        c.assertIsNode(node, 'instructionNode');
        return { ...node, optionalAccountStrategy: 'omitted' };
      },
    },
  ])
);

// Render JavaScript.
const jsClient = path.join(workingDirectory, 'clients', 'js');
codama.accept(
  renderJavaScriptVisitor(path.join(jsClient, 'src', 'generated'), {
    prettierOptions: JSON.parse(
      fs.readFileSync(path.join(jsClient, '.prettierrc.json'), 'utf-8')
    ),
  })
);

// Remove the stake account from the accounts since the Rust client
// provides its own implementation.
codama.update(
  c.updateAccountsVisitor({
    stakeStateAccount: { delete: true },
  })
);

// Render Rust.
const rustClient = path.join(workingDirectory, 'clients', 'rust');
codama.accept(
  renderRustVisitor(path.join(rustClient, 'src', 'generated'), {
    formatCode: true,
    crateFolder: rustClient,
    anchorTraits: false,
    toolchain: getToolchainArgument('format'),
    traitOptions: {
      baseDefaults: [
        'borsh::BorshSerialize',
        'borsh::BorshDeserialize',
        'serde::Serialize',
        'serde::Deserialize',
        'Clone',
        'Debug',
        // 'Eq', <- Remove 'Eq' from the default traits.
        'PartialEq',
      ],
    },
  })
);
