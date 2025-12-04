//! Codama IDL generation binary.
#[cfg(feature = "codama")]
use {
    codama::Codama,
    std::{env, fs, path::Path},
};

#[cfg(feature = "codama")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate IDL.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let crate_path = Path::new(&manifest_dir);
    let codama = Codama::load(crate_path)?;
    let idl_json = codama.get_json_idl()?;

    // Parse and format the JSON with pretty printing.
    let parsed: serde_json::Value = serde_json::from_str(&idl_json)?;
    let mut formatted_json = serde_json::to_string_pretty(&parsed)?;
    formatted_json.push('\n');

    // Write IDL file.
    let idl_path = Path::new(&manifest_dir).join("idl.json");
    fs::write(&idl_path, formatted_json)?;

    println!("IDL written to: {}", idl_path.display());
    Ok(())
}

#[cfg(not(feature = "codama"))]
fn main() {
    println!("Codama is not enabled");
}
