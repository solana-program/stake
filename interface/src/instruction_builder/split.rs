use {
    crate::instruction::StakeInstruction,
    crate::program::ID,
    crate::state::StakeStateV2,
    solana_instruction::{AccountMeta, Instruction},
    solana_pubkey::Pubkey,
};

#[derive(Debug, Clone)]
struct AllocateWithSeedArgs<'a> {
    base: &'a Pubkey,
    seed: &'a str,
}

#[derive(Debug, Clone)]
enum CreationInfo<'a> {
    None,
    AllocateAndAssign,
    AllocateWithSeed(AllocateWithSeedArgs<'a>),
}

#[derive(Debug, Clone)]
pub struct SplitBuilder<'a> {
    stake_pubkey: &'a Pubkey,
    stake_authority_pubkey: &'a Pubkey,
    split_stake_pubkey: &'a Pubkey,
    lamports: u64,
    creation_info: CreationInfo<'a>,
}

impl<'a> SplitBuilder<'a> {
    pub fn new(
        stake_pubkey: &'a Pubkey,
        stake_authority_pubkey: &'a Pubkey,
        split_stake_pubkey: &'a Pubkey,
        lamports: u64,
    ) -> Self {
        Self {
            stake_pubkey,
            stake_authority_pubkey,
            split_stake_pubkey,
            lamports,
            creation_info: CreationInfo::None,
        }
    }

    pub fn with_allocate_and_assign(&mut self) -> &mut Self {
        self.creation_info = CreationInfo::AllocateAndAssign;
        self
    }

    pub fn with_allocate_with_seed(&mut self, base: &'a Pubkey, seed: &'a str) -> &mut Self {
        self.creation_info = CreationInfo::AllocateWithSeed(AllocateWithSeedArgs { base, seed });
        self
    }

    pub fn build(&self) -> Vec<Instruction> {
        let mut ixs = Vec::new();

        match &self.creation_info {
            CreationInfo::AllocateAndAssign => {
                ixs.push(solana_system_interface::instruction::allocate(
                    self.split_stake_pubkey,
                    StakeStateV2::size_of() as u64,
                ));
                ixs.push(solana_system_interface::instruction::assign(
                    self.split_stake_pubkey,
                    &ID,
                ));
            }
            CreationInfo::AllocateWithSeed(args) => {
                ixs.push(solana_system_interface::instruction::allocate_with_seed(
                    self.split_stake_pubkey,
                    args.base,
                    args.seed,
                    StakeStateV2::size_of() as u64,
                    &ID,
                ));
            }
            CreationInfo::None => {}
        }

        let split_ix = Instruction::new_with_bincode(
            ID,
            &StakeInstruction::Split(self.lamports),
            vec![
                AccountMeta::new(*self.stake_pubkey, false),
                AccountMeta::new(*self.split_stake_pubkey, false),
                AccountMeta::new_readonly(*self.stake_authority_pubkey, true),
            ],
        );
        ixs.push(split_ix);

        ixs
    }
}
