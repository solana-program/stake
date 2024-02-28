use {
    num_traits::cast::ToPrimitive,
    solana_program::{
        clock::Epoch, instruction::InstructionError, program_error::ProgramError,
        stake::instruction::StakeError,
    },
};

pub(crate) mod delegate;
pub(crate) use delegate::*;

pub(crate) mod split;
pub(crate) use split::*;

pub(crate) mod merge;
pub(crate) use merge::*;

pub(crate) fn checked_add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(ProgramError::InsufficientFunds)
}

// XXX impl from<StakeError> for ProgramError. also idk if this is correct
// i just want to keep the same errors in-place and then clean up later, instead of needing to hunt down the right ones
// XXX there should also be a better wrapper for TryFrom<InstructionError> for ProgramError
// like, if theres a matching error do the conversion, if custom do the custom conversion
// otherwise unwrap into an error cnoversion error maybe. idk

// TODO pr monorepo to imple From<StakeError> for ProgramError, make sure this is how uhhh
// impl From<SinglePoolError> for ProgramError {
//     fn from(e: SinglePoolError) -> Self {
//         ProgramError::Custom(e as u32)
//     }
// }
// ok looks fine lol. also figure out a better way for instruction error conversion its so annoying
pub(crate) trait TurnInto {
    fn turn_into(self) -> ProgramError;
}

impl TurnInto for StakeError {
    fn turn_into(self) -> ProgramError {
        ProgramError::Custom(self.to_u32().unwrap())
    }
}

impl TurnInto for InstructionError {
    fn turn_into(self) -> ProgramError {
        match ProgramError::try_from(self) {
            Ok(program_error) => program_error,
            Err(e) => panic!("HANA error conversion failed: {:?}", e),
        }
    }
}

// XXX this is stubbed out because it depends on invoke context and feature set
// what this does is calls a feature_set.rs function also called `new_warmup_cooldown_rate_epoch`
// which gets the slot that the `reduce_stake_warmup_cooldown` feature was activated at
// and then passes the slot to `epoch_schedule.get_epoch()` to convert it to an epoch
// in other words `new_warmup_cooldown_rate_epoch` does exactly what it says
// this results in a Option<Epoch> that gets passed into various stake functions
// get activating/deactivating, calculate rewards, etc
//
// ok so that means if the feature isnt active we return None. easy
// if the feature *is* active then its tricky if we dont have access to the featureset
// EpochSchedule has a sysvar get impl but we would need to... hardcode the epochs for the networks? idk
//
// TODO i need to look at wtf this number is actually used for
// presumbly it is not as simple as just "are we active yet" otherwise there wouldnt be this dance
// i assume the intent is stake history behaves differently before and after the cutover
// but i *believe* all this stuff is to change it from "we have a 25% deactivation cap defined by stake config"
// to "we have a 7% deactivation cap hardcoded" so we could deploy a second feature to get rid of the plumbing
// once history has an epoch in it when there is less than 7% deactivation? idk
// history is fucking confusing to me still
// maybe i should write a post about it and have someone just factcheck me so i understand lol
//
// XXX ok the way this works is like...
// say its current epoch 100. and the new rate was activated in epoch 50
// `stake_and_activating()` first gets the activation epoch of the delegation, assuming we are ahead of it
// then the loop is used to cumulatively sum the effective stake of the delegation at each subsequent epoch
// basically, the warmup rate sets a cap, 25% or 9% of total effective stake at that time
// so we... only sort of need to preserve the new warmup cooldown
// on the one hand, keeping it ensures all historical calculations are correct
// but on the other, they dont need to be correct as far as i can tell?
// all we really care about is whether stake is active. so as long as theres one epoch below the threshhold
// we can say "ok all stake was in its proper state by this point, whenever that was"
pub(crate) fn new_warmup_cooldown_rate_epoch() -> Option<Epoch> {
    Some(1)
}
