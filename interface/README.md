<p align="center">
  <a href="https://solana.com">
    <img alt="Solana" src="https://github.com/user-attachments/assets/534af75d-6347-48dc-8943-129423b2ba63" height="80" />
  </a>
</p>

# Solana Stake Interface

This crate contains instructions and constructors for interacting with the [Stake program](https://docs.anza.xyz/runtime/programs/#stake-program).

The Stake program can be used to create and manage accounts representing stake and rewards for delegations to validators.

## Getting Started

From your project folder:

```bash
cargo add solana-stake-interface --features bincode
```

This will add the `solana-stake-interface` dependency with the `bincode` feature enabled to your `Cargo.toml` file. The `bincode` feature contains the instruction constructors to create instructions for the Stake program.

The `wincode` feature offers the same constructors over the [`wincode`](https://docs.rs/wincode) codec. wincode encodes byte-for-byte like bincode. The feature also derives `SchemaRead`/`SchemaWrite` on the instruction and state types, so callers decode them without bincode. Enabling both features makes the constructors use wincode.

## Documentation

Read more about the Stake program on the crate [documentation](https://docs.rs/solana-stake-interface/latest/solana_stake_interface/).