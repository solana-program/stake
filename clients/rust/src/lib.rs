mod generated;
pub mod hooked;

pub use {
    generated::{programs::STAKE_ID as ID, *},
    hooked as accounts,
};
