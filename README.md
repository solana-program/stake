# Stake Program

Solana Core BPF Stake Program.

| Information | Account Address |
| --- | --- |
| Stake Program | `Stake11111111111111111111111111111111111111` |

## Overview

This repository contains the BPF port of the native Agave Stake Program, along with the official Rust interface and Codama-generated Rust and Javascript clients for interacting with it. The Stake Program is what allows stake accounts to be created, delegated, split, merged, and so forth.

Historically, the Stake Program was built into the Agave validator client as a pseudo-program. This would necessitate that any future validator client reimplement it exactly. To ease the development of new validator clients, the Stake Program has been converted to an ordinary BPF program which can be invoked like any normal program. This BPF Stake Program is now live on all clusters.

## Security Audits

The BPF Stake Program has received one external audit:

* Zellic (2025-03-12)
    - Review commit hash [`5ec49ccb`](https://github.com/solana-program/stake/commit/5ec49ccb08c3e588940a2038c99efc7acf563b4a)
    - Final report <https://github.com/anza-xyz/security-audits/blob/master/core-bpf/ZellicStakeProgramAudit-2025-03-12.pdf>

## Building and Verifying

To build the BPF Stake Program, you can run `cargo-build-sbf` or use the Makefile
command:

```console
cargo build-sbf --manifest-path program/Cargo.toml
make build-sbf-program
```

The BPF program deployed on all clusters is built with [solana-verify](https://solana.com/developers/guides/advanced/verified-builds). It may be verified independently by comparing the output of:

```console
solana-verify get-program-hash -um Stake11111111111111111111111111111111111111
```

with:

```console
solana-verify build --library-name solana_stake_program
```

It is possible for a solana-verify version mismatch to affect the hash; [BPF Stake Program 1.0.0](https://github.com/solana-program/stake/releases/tag/program%40v1.0.0) was built with solana-verify 0.4.6.

## Interface

Instructions, errors, and assorted structs related to the stake program, which used to live in the Solana SDK repo, now live here. For more, see [docs.rs](https://docs.rs/solana-stake-interface/latest/solana_stake_interface/).

## Compute Units

Previously, the Stake Program was essentially free, costing 1500 Compute Units irrespective of any work the program actually did. As a normal BPF program, the Stake Program now pays Compute Units as any other program.

For the initial 1.0.0 release, we followed the existing Agave code as closely as possible, to minimize the possibility of introducing any bugs or changing any behaviors in the port because of the total rewrite of the calling interface the port necessitated. This means the existing program is expected to be non-optimal. With the ability to test more thoroughly against this 1.0.0 version, we expect to be able to optimize these costs in the future.

Based on the sample invocations in `program/tests/interface.rs`, approximate costs as of 2025-07-28 are as follows. These should be treated as baselines and are rounded to thousands; instructions may do less or more work depending on arguments and account states.

| Instruction | Estimated Cost |
| --- | --- |
| `Initialize` | 9000 |
| `Authorize` | 12000 |
| `DelegateStake` | 15000 |
| `Split` | 20000 |
| `Withdraw` | 8000 |
| `Deactivate` | 13000 |
| `SetLockup` | 11000 |
| `Merge` | 21000 |
| `AuthorizeWithSeed` | 14000 |
| `InitializeChecked` | 6000 |
| `AuthorizeChecked` | 12000 |
| `AuthorizeCheckedWithSeed` | 14000 |
| `SetLockupChecked` | 11000 |
| `GetMinimumDelegation` | (negligible) |
| `DeactivateDelinquent` | 18000 |
| `MoveStake` | 25000 |
| `MoveLamports` | 15000 |
