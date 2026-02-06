import * as c from 'codama';

export default {
    idl: 'interface/idl.json',
    before: [
        {
            from: 'codama#updateProgramsVisitor',
            args: [{ solanaStakeInterface: { name: 'stake' } }],
        },
        {
            from: 'codama#updateInstructionsVisitor',
            args: [{ redelegate: { delete: true } }],
        },
        {
            from: 'codama#updateDefinedTypesVisitor',
            args: [
                {
                    lockupArgs: { name: 'lockupParams' },
                    lockupCheckedArgs: { name: 'lockupCheckedParams' },
                    authorizeWithSeedArgs: { name: 'authorizeWithSeedParams' },
                    authorizeCheckedWithSeedArgs: { name: 'authorizeCheckedWithSeedParams' },
                },
            ],
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
                                    // Add UnixTimestamp type alias
                                    c.definedTypeNode({ name: 'unixTimestamp', type: c.numberTypeNode('i64') }),
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
                        // enum discriminator -> u32
                        select: '[definedTypeNode]stakeState.[enumTypeNode]',
                        transform: node => {
                            c.assertIsNode(node, 'enumTypeNode');
                            return { ...node, size: c.numberTypeNode('u32') };
                        },
                    },
                    {
                        // enum discriminator -> u32
                        select: '[definedTypeNode]stakeStateV2.[enumTypeNode]',
                        transform: node => {
                            c.assertIsNode(node, 'enumTypeNode');
                            return { ...node, size: c.numberTypeNode('u32') };
                        },
                    },
                    {
                        // Use omitted optional account strategy for all instructions.
                        select: '[instructionNode]',
                        transform: node => {
                            c.assertIsNode(node, 'instructionNode');
                            return {
                                ...node,
                                optionalAccountStrategy: 'omitted',
                                arguments: node.arguments.map(arg =>
                                    arg.name === 'discriminator' ? { ...arg, type: c.numberTypeNode('u32') } : arg,
                                ),
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
                'clients/js/src/generated',
                {
                    packageFolder: 'clients/js',
                    syncPackageJson: true,
                },
            ],
        },
        rust: [
            {
                from: 'codama#updateAccountsVisitor',
                args: [{ stakeStateAccount: { delete: true } }],
            },
            {
                from: '@codama/renderers-rust',
                args: [
                    'clients/rust/src/generated',
                    {
                        anchorTraits: false,
                        crateFolder: 'clients/rust',
                        formatCode: true,
                        toolchain: '+nightly-2025-02-16',
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
                    },
                ],
            },
        ],
    },
};
