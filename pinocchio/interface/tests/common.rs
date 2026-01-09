#![allow(dead_code)]

use {
    bincode::Options,
    core::mem::size_of,
    p_stake_interface::state::{StakeStateV2Layout, StakeStateV2Tag},
    solana_stake_interface::state::StakeStateV2 as OldStakeStateV2,
};

pub fn bincode_opts() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

pub fn serialize_old(state: &OldStakeStateV2) -> Vec<u8> {
    let mut data = bincode_opts().serialize(state).unwrap();
    data.resize(size_of::<StakeStateV2Layout>(), 0);
    data
}

pub fn deserialize_old(data: &[u8]) -> OldStakeStateV2 {
    bincode_opts().deserialize::<OldStakeStateV2>(data).unwrap()
}

pub fn empty_state_bytes(tag: StakeStateV2Tag) -> [u8; 200] {
    let mut data = [0u8; size_of::<StakeStateV2Layout>()];
    let mut slice = &mut data[..StakeStateV2Tag::TAG_LEN];
    wincode::serialize_into(&mut slice, &tag).unwrap();
    data
}
