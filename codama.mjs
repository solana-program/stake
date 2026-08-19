import * as c from 'codama';

export default {
    idl: 'idl.json',
    before: [
        {
            from: 'codama#updateInstructionsVisitor',
            args: [{ redelegate: { delete: true } }],
        },
        'codama#unwrapInstructionArgsDefinedTypesVisitor',
        'codama#flattenInstructionDataArgumentsVisitor',
        {
            from: 'codama#bottomUpTransformerVisitor',
            args: [
                [
                    {
                        select: '[programNode]',
                        transform: node => {
                            c.assertIsNode(node, 'programNode');
                            return {
                                ...node,
                                definedTypes: [
                                    // Add Epoch type alias
                                    c.definedTypeNode({ name: 'epoch', type: c.numberTypeNode('u64') }),
                                    // Add UnixTimestamp type alias, displayed as a date-time
                                    c.definedTypeNode({
                                        name: 'unixTimestamp',
                                        type: c.numberTypeNode('i64', 'le', {
                                            display: c.dateTimeNumberDisplayNode({}),
                                        }),
                                    }),
                                    ...node.definedTypes,
                                ],
                            };
                        },
                    },
                ],
            ],
        },
        {
            from: 'codama#bottomUpTransformerVisitor',
            args: [
                [
                    {
                        select: '[programNode]',
                        transform: node => {
                            c.assertIsNode(node, 'programNode');
                            return {
                                ...node,
                                accounts: [
                                    ...(node.accounts ?? []),
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
                ],
            ],
        },
    ],
    scripts: {
        js: {
            from: '@codama/renderers-js',
            args: [
                'clients/js',
                {
                    kitImportStrategy: 'rootOnly',
                    syncPackageJson: true,
                    prettierOptions: {
                        arrowParens: 'avoid',
                        printWidth: 120,
                        singleQuote: true,
                        tabWidth: 4,
                        trailingComma: 'all',
                    },
                },
            ],
        },
        rust: [
            {
                from: 'codama#updateAccountsVisitor',
                args: [{ stakeStateAccount: { delete: true } }],
            },
            {
                // Codama node helpers omit empty account lists when rebuilding nodes,
                // but the Rust renderer templates expect them to be present.
                from: 'codama#bottomUpTransformerVisitor',
                args: [
                    [
                        {
                            select: '[instructionNode]',
                            transform: node => {
                                c.assertIsNode(node, 'instructionNode');
                                return { ...node, accounts: node.accounts ?? [] };
                            },
                        },
                    ],
                ],
            },
            {
                from: '@codama/renderers-rust',
                args: [
                    'clients/rust',
                    {
                        anchorTraits: false,
                        formatCode: true,
                        toolchain: '+nightly-2026-01-22',
                        traitOptions: {
                            baseDefaults: [
                                'borsh::BorshSerialize',
                                'borsh::BorshDeserialize',
                                'Clone',
                                'Debug',
                                // 'Eq', <- Remove 'Eq' from the default traits.
                                'PartialEq',
                            ],
                        },
                    },
                ],
            },
        ],
    },
};
