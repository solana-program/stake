use {bincode::Options, solana_stake_interface::state::StakeStateV2 as LegacyStakeStateV2};

const LEGACY_LAYOUT_LEN: usize = 200;

pub fn bincode_opts() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

pub fn serialize_legacy(state: &LegacyStakeStateV2) -> Vec<u8> {
    let mut data = bincode_opts().serialize(state).unwrap();
    assert!(data.len() <= LEGACY_LAYOUT_LEN);
    data.resize(LEGACY_LAYOUT_LEN, 0);
    data
}

pub fn serialize_legacy_unpadded(state: &LegacyStakeStateV2) -> Vec<u8> {
    let data = bincode_opts().serialize(state).unwrap();
    assert!(data.len() <= LEGACY_LAYOUT_LEN);
    data
}

pub fn deserialize_legacy(data: &[u8]) -> LegacyStakeStateV2 {
    bincode_opts().deserialize(data).unwrap()
}
