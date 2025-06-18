use {
    crate::instruction::StakeInstruction,
    crate::program::ID,
    crate::state::{Authorized, Lockup, StakeStateV2},
    solana_instruction::{AccountMeta, Instruction},
    solana_pubkey::Pubkey,
};

#[derive(Debug, Clone)]
struct CreateAccountArgs<'a> {
    from_pubkey: &'a Pubkey,
    lamports: u64,
}

#[derive(Debug, Clone)]
struct CreateAccountWithSeedArgs<'a> {
    from_pubkey: &'a Pubkey,
    base: &'a Pubkey,
    seed: &'a str,
    lamports: u64,
}

#[derive(Debug, Clone)]
enum CreationInfo<'a> {
    None,
    Simple(CreateAccountArgs<'a>),
    WithSeed(CreateAccountWithSeedArgs<'a>),
}

#[derive(Debug, Clone)]
pub struct InitializeBuilder<'a> {
    stake_pubkey: &'a Pubkey,
    authorized: &'a Authorized,
    lockup: &'a Lockup,
    creation_info: CreationInfo<'a>,
}

impl<'a> InitializeBuilder<'a> {
    pub fn new(stake_pubkey: &'a Pubkey, authorized: &'a Authorized, lockup: &'a Lockup) -> Self {
        Self {
            stake_pubkey,
            authorized,
            lockup,
            creation_info: CreationInfo::None,
        }
    }

    pub fn create_account(&mut self, from_pubkey: &'a Pubkey, lamports: u64) -> &mut Self {
        self.creation_info = CreationInfo::Simple(CreateAccountArgs {
            from_pubkey,
            lamports,
        });
        self
    }

    pub fn create_account_with_seed(
        &mut self,
        from_pubkey: &'a Pubkey,
        base: &'a Pubkey,
        seed: &'a str,
        lamports: u64,
    ) -> &mut Self {
        self.creation_info = CreationInfo::WithSeed(CreateAccountWithSeedArgs {
            from_pubkey,
            base,
            seed,
            lamports,
        });
        self
    }

    pub fn build(&self) -> Vec<Instruction> {
        let mut ixs = Vec::new();

        match &self.creation_info {
            CreationInfo::Simple(args) => {
                ixs.push(solana_system_interface::instruction::create_account(
                    args.from_pubkey,
                    self.stake_pubkey,
                    args.lamports,
                    StakeStateV2::size_of() as u64,
                    &ID,
                ));
            }
            CreationInfo::WithSeed(args) => {
                ixs.push(
                    solana_system_interface::instruction::create_account_with_seed(
                        args.from_pubkey,
                        self.stake_pubkey,
                        args.base,
                        args.seed,
                        args.lamports,
                        StakeStateV2::size_of() as u64,
                        &ID,
                    ),
                );
            }
            CreationInfo::None => {}
        }

        let initialize_ix = Instruction::new_with_bincode(
            ID,
            &StakeInstruction::Initialize(*self.authorized, *self.lockup),
            vec![
                AccountMeta::new(*self.stake_pubkey, false),
                AccountMeta::new_readonly(crate::instruction_builder::RENT_ID, false),
            ],
        );
        ixs.push(initialize_ix);

        ixs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction_builder::StakeInstructionBuilder;

    #[test]
    fn test_works() {
        let from_pubkey = Pubkey::new_unique();
        let stake_pubkey = Pubkey::new_unique();
        let authorized = Authorized {
            staker: from_pubkey,
            withdrawer: from_pubkey,
        };
        let lockup = Lockup::default();
        let lamports = 1_000_000_000;

        let ixs = StakeInstructionBuilder::initialize(&stake_pubkey, &authorized, &lockup).build();

        assert_eq!(ixs.len(), 1);

        let ixs = StakeInstructionBuilder::initialize(&stake_pubkey, &authorized, &lockup)
            .create_account(&from_pubkey, lamports)
            .build();

        assert_eq!(ixs.len(), 2);
    }
}
