#!/usr/bin/env zx
import 'zx/globals';
import * as c from 'codama';
import { renderVisitor as renderJavaScriptVisitor } from '@codama/renderers-js';
import { renderVisitor as renderRustVisitor } from '@codama/renderers-rust';
import { getToolchainArgument, workingDirectory } from './utils.mts';

// Load the auto-generated IDL from Codama macros
const idl = JSON.parse(
  fs.readFileSync(path.join(workingDirectory, 'interface', 'idl.json'), 'utf-8')
);
const codama = c.createFromRoot(idl);

// Rename the program from solanaStakeInterface to stake
codama.update(
  c.updateProgramsVisitor({
    solanaStakeInterface: { name: 'stake' },
  })
);

// Add type aliases for semantic external types
codama.update(
  c.bottomUpTransformerVisitor([
    {
      select: '[programNode]',
      transform: (node) => {
        c.assertIsNode(node, 'programNode');
        return {
          ...node,
          definedTypes: [
            // Add Epoch type alias
            c.definedTypeNode({
              name: 'epoch',
              type: c.numberTypeNode('u64'),
            }),
            // Add UnixTimestamp type alias
            c.definedTypeNode({
              name: 'unixTimestamp',
              type: c.numberTypeNode('i64'),
            }),
            ...node.definedTypes,
          ],
        };
      },
    },
  ])
);

// Apply transformations to the IDL
codama.update(
  c.bottomUpTransformerVisitor([
    {
      select: '[programNode]',
      transform: (node) => {
        c.assertIsNode(node, 'programNode');
        return {
          ...node,
          accounts: [
            ...node.accounts,
            // Stake account wrapper for client convenience
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
        };
      },
    },
    {
      // Use omitted optional account strategy for all instructions
      select: '[instructionNode]',
      transform: (node) => {
        c.assertIsNode(node, 'instructionNode');
        return { ...node, optionalAccountStrategy: 'omitted' };
      },
    },
  ])
);

// Render JavaScript client
const jsClient = path.join(workingDirectory, 'clients', 'js');
codama.accept(
  renderJavaScriptVisitor(path.join(jsClient, 'src', 'generated'), {
    prettierOptions: JSON.parse(
      fs.readFileSync(path.join(jsClient, '.prettierrc.json'), 'utf-8')
    ),
  })
);

// Remove the stake account from the accounts since the Rust client
// provides its own implementation in the hooked module
codama.update(
  c.updateAccountsVisitor({
    stakeStateAccount: { delete: true },
  })
);

// Render Rust client
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
        'PartialEq',
      ],
    },
  })
);

