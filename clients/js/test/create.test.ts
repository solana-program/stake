import test from 'ava';
import {
  createDefaultSolanaClient,
  generateKeyPairSignerWithSol,
} from './_setup';

test('it creates a keypair', async (t) => {
  // Given an authority key pair with some SOL.
  const client = createDefaultSolanaClient();
  const authority = await generateKeyPairSignerWithSol(client);

  t.true(authority.address.length > 0);
});
