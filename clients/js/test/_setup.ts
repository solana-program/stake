import path from 'node:path';

import { createClient, lamports } from '@solana/kit';
import { litesvm } from '@solana/kit-plugin-litesvm';
import { airdropSigner, generatedSigner } from '@solana/kit-plugin-signer';

import { STAKE_PROGRAM_ADDRESS, stakeProgram } from '../src';

const STAKE_BINARY_PATH = path.resolve(__dirname, '..', '..', '..', 'target', 'deploy', 'solana_stake_program.so');

export const createTestClient = () => {
    return createClient()
        .use(generatedSigner())
        .use(litesvm())
        .use(airdropSigner(lamports(1_000_000_000n)))
        .use(client => {
            // Load the stake program into the LiteSVM instance from its compiled
            // `.so` file. This must run after the `litesvm()` plugin so that
            // `client.svm` is available.
            client.svm.addProgramFromFile(STAKE_PROGRAM_ADDRESS, STAKE_BINARY_PATH);
            return client;
        })
        .use(stakeProgram());
};
