import { expect, it } from 'vitest';

import { createTestClient } from './_setup';

it('sets up a LiteSVM client with the stake program', async () => {
    // Given a test client whose payer is funded with SOL.
    const client = await createTestClient();

    // Then the client exposes the stake program plugin.
    expect(client.stake).toBeDefined();

    // And the payer was funded via LiteSVM.
    const { value: balance } = await client.rpc.getBalance(client.payer.address).send();
    expect(balance).toBe(1_000_000_000n);
});
