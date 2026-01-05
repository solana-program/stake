//! Zero-copy stake state types

mod entrypoint;
mod layout;
mod pod;
mod view;
mod writer;

pub use {
    entrypoint::StakeStateV2,
    layout::*,
    pod::*,
    view::*,
    writer::{StakeStateV2ViewMut, StakeStateV2Writer},
};
