use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use std::fs::create_dir_all;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use tar::Archive;

/// Dump of all stake accounts as of Nov '25
const MAINNET_STAKE_ACCOUNTS_URL: &str =
    "https://github.com/solana-program/stake/releases/download/program%40v1.0.1/mainnet_stake_accounts.tar.gz";

/// Ensures the stake accounts snapshot is available and returns the path to the bin file.
pub fn ensure_stake_accounts_bin_available() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let artifacts_dir = manifest_dir.join("artifacts");
    let bin_path = artifacts_dir.join("stake_accounts.bin");

    // Do not re-download if already present
    if bin_path.exists() {
        return bin_path;
    }

    create_dir_all(&artifacts_dir).expect("Failed to create artifacts dir");
    download_and_unpack_tar_gz(
        MAINNET_STAKE_ACCOUNTS_URL,
        &artifacts_dir,
        "stake accounts snapshot",
    );

    bin_path
}

fn download_and_unpack_tar_gz(url: &str, dest: &Path, label: &str) {
    print!("{label} not found locally. Downloading from {url}... ");
    std::io::stdout().flush().unwrap();

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .expect("Failed to build HTTP client");

    let response = client
        .get(url)
        .send()
        .expect("Failed to send request")
        .error_for_status()
        .expect("HTTP request failed");
    let bytes = response.bytes().expect("Failed to read body");

    println!("Done.");
    println!("Extracting {}...", label);
    unpack_tar_gz(Cursor::new(bytes), dest);
    println!("Extraction complete.");
}

fn unpack_tar_gz<R: Read>(reader: R, dest: &Path) {
    let decoder = GzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    archive.unpack(dest).expect("Failed to unpack archive")
}
