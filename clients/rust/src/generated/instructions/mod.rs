//! This code was AUTOGENERATED using the codama library.
//! Please DO NOT EDIT THIS FILE, instead use visitors
//! to add features, then rerun codama to update it.
//!
//! <https://github.com/codama-idl/codama>
//!

pub(crate) mod r#authorize;
pub(crate) mod r#authorize_checked;
pub(crate) mod r#authorize_checked_with_seed;
pub(crate) mod r#authorize_with_seed;
pub(crate) mod r#deactivate;
pub(crate) mod r#deactivate_delinquent;
pub(crate) mod r#delegate_stake;
pub(crate) mod r#get_minimum_delegation;
pub(crate) mod r#initialize;
pub(crate) mod r#initialize_checked;
pub(crate) mod r#merge;
pub(crate) mod r#set_lockup;
pub(crate) mod r#set_lockup_checked;
pub(crate) mod r#split;
pub(crate) mod r#withdraw;

pub use self::{
    r#authorize::*, r#authorize_checked::*, r#authorize_checked_with_seed::*,
    r#authorize_with_seed::*, r#deactivate::*, r#deactivate_delinquent::*, r#delegate_stake::*,
    r#get_minimum_delegation::*, r#initialize::*, r#initialize_checked::*, r#merge::*,
    r#set_lockup::*, r#set_lockup_checked::*, r#split::*, r#withdraw::*,
};
