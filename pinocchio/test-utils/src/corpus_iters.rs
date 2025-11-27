use crate::request::ensure_stake_accounts_bin_available;
use bincode::Options;
use serde::Deserialize;
use solana_account::Account;
use solana_address::Address;
use std::fs::File;
use std::io::BufReader;

#[derive(Deserialize)]
struct StoredStakeAccount {
    _address: Address,
    account: Account,
}

/// Iterate over snapshot of all mainnet stake accounts and invoke callback on account data
pub fn iter_mainnet_stake_accounts_data<F>(mut callback: F)
where
    F: FnMut(&[u8]),
{
    let bin_path = ensure_stake_accounts_bin_available();
    let file = File::open(&bin_path).expect("Failed to open stake accounts bin file");
    let mut reader = BufReader::new(file);

    loop {
        match bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .deserialize_from::<_, StoredStakeAccount>(&mut reader)
        {
            Ok(account) => callback(&account.account.data),
            Err(err) => match *err {
                // A bincode error wrapping `UnexpectedEof` is treated as normal end-of-file and
                // terminates iteration successfully.
                bincode::ErrorKind::Io(ref io_err)
                    if io_err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                _ => {
                    panic!("Failed to deserialize stake account in corpus: {}", err);
                }
            },
        }
    }
}
