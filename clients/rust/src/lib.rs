#![allow(non_local_definitions)] // <-- Rustc warning on `FromPrimitive`

#[allow(clippy::arithmetic_side_effects)]
mod generated;
mod hooked;

pub use {
    generated::{programs::STAKE_ID as ID, *},
    hooked::StakeStateAccount,
};
