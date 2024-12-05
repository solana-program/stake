mod generated;
mod hooked;

pub use {
    generated::{programs::STAKE_ID as ID, *},
    hooked::StakeStateAccount,
};
