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

codama.accept(c.consoleLogVisitor(c.getDebugStringVisitor({ indent: true })));

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
      // Epoch -> u64
      select: (node) => {
        return (
          c.isNode(node, "structFieldTypeNode") &&
          c.isNode(node.type, "definedTypeLinkNode") &&
          node.type.name === "epoch"
        );
      },
      transform: (node) => {
        c.assertIsNode(node, "structFieldTypeNode");
        return {
          ...node,
          type: c.numberTypeNode("u64"),
        };
      },
    },
    {
      // UnixTimestamp -> i64
      select: (node) => {
        return (
          c.isNode(node, "structFieldTypeNode") &&
          c.isNode(node.type, "definedTypeLinkNode") &&
          node.type.name === "unixTimestamp"
        );
      },
      transform: (node) => {
        c.assertIsNode(node, "structFieldTypeNode");
        return {
          ...node,
          type: c.numberTypeNode("i64"),
        };
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
  })
);
