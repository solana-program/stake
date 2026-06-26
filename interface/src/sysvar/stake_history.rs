//! History of stake activations and de-activations.
//!
//! The _stake history sysvar_ provides access to the
//! [`StakeHistory`](crate::stake_history::StakeHistory) type.
//!
//! The `Sysvar::get` method always returns
//! [`ProgramError::UnsupportedSysvar`], and in practice the data size of this
//! sysvar is too large to process on chain. One can still use the
//! [`SysvarId::id`] and [`SysvarId::check_id`] methods in an on-chain program,
//! and it can be accessed off-chain through RPC.
//!
//! [`ProgramError::UnsupportedSysvar`]: https://docs.rs/solana-program-error/latest/solana_program_error/enum.ProgramError.html#variant.UnsupportedSysvar
//! [`SysvarId::id`]: https://docs.rs/solana-sysvar-id/latest/solana_sysvar_id/trait.SysvarId.html
//! [`SysvarId::check_id`]: https://docs.rs/solana-sysvar-id/latest/solana_sysvar_id/trait.SysvarId.html#tymethod.check_id
//!
//! # Examples
//!
//! Calling via the RPC client:
//!
//! ```
//! # use solana_example_mocks::{solana_account, solana_rpc_client};
//! # use solana_stake_interface::{stake_history::StakeHistory, sysvar::stake_history};
//! # use solana_account::Account;
//! # use solana_rpc_client::rpc_client::RpcClient;
//! # use anyhow::Result;
//! #
//! fn print_sysvar_stake_history(client: &RpcClient) -> Result<()> {
//! #   client.set_get_account_response(stake_history::ID, Account {
//! #       lamports: 114979200,
//! #       data: vec![0, 0, 0, 0, 0, 0, 0, 0],
//! #       owner: solana_sdk_ids::system_program::ID,
//! #       executable: false,
//! #   });
//! #
//!     let stake_history = client.get_account(&stake_history::ID)?;
//!     let data: StakeHistory = bincode::deserialize(&stake_history.data)?;
//!
//!     Ok(())
//! }
//! #
//! # let client = RpcClient::new(String::new());
//! # print_sysvar_stake_history(&client)?;
//! #
//! # Ok::<(), anyhow::Error>(())
//! ```

pub use solana_stake_history::sysvar::{check_id, id, StakeHistorySysvar, ID};
