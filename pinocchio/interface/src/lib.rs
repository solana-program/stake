#[cfg(test)]
fn num_of_mainnet_stake_accounts(num: u64) -> bool {
    num == 1_183_243
}

#[cfg(test)]
mod tests {
    use {
        crate::num_of_mainnet_stake_accounts, p_stake_test_utils::iter_mainnet_stake_accounts_data,
    };

    #[test]
    fn test_corpus_deserialization() {
        let mut count = 0;
        iter_mainnet_stake_accounts_data(|_data| {
            count += 1;
        });
        println!("Corpus length: {}", count);
        assert!(num_of_mainnet_stake_accounts(count));
    }
}
