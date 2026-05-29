import { expect, it } from 'vitest';
import { createDefaultSolanaClient, generateKeyPairSignerWithSol } from './_setup';

it('creates a keypair', async () => {
    // Given an authority key pair with some SOL.
    const client = createDefaultSolanaClient();
    const authority = await generateKeyPairSignerWithSol(client);

    expect(authority.address.length > 0).toBe(true);
});
