//! Program entrypoint

use {
    crate::processor::Processor,
    solana_account_info::AccountInfo,
    solana_msg::msg,
    solana_program_entrypoint::{entrypoint, ProgramResult},
    solana_pubkey::Pubkey,
    solana_security_txt::security_txt,
};

entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if let Err(error) = Processor::process(program_id, accounts, instruction_data) {
        msg!("ERROR: {}", error);
        Err(error)
    } else {
        Ok(())
    }
}

security_txt! {
    // Required fields
    name: "Solana Stake Program",
    project_url: "https://solana.com/docs/core/programs/builtin-programs#all-core-programs",
    contacts: "link:https://github.com/solana-program/stake/security/advisories/new,email:security@anza.xyz,discord:https://discord.gg/solana",
    policy: "https://github.com/solana-program/stake/blob/main/SECURITY.md",

    // Optional Fields
    preferred_languages: "en",
    source_code: "https://github.com/solana-program/stake/tree/main/program",
    source_release: concat!("program@v", env!("CARGO_PKG_VERSION")),
    auditors: "https://github.com/solana-program/stake/tree/main?tab=readme-ov-file#security-audits"
}
