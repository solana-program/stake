#![allow(non_local_definitions)] // <-- Rustc warning on `FromPrimitive`

mod generated;
mod hooked;

pub use {
    generated::{programs::STAKE_ID as ID, *},
    hooked::StakeStateAccount,
};
