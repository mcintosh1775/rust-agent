use anyhow::{anyhow, Context, Result};
use nostr::{Keys, ToBech32};
use serde_json::json;
use std::env;

fn print_help() {
    println!("secureagnt-nostr-keygen");
    println!("Generate one Nostr keypair for agent/operator provisioning.");
    println!();
    println!("Usage:");
    println!("  secureagnt-nostr-keygen [--json]");
    println!();
    println!("Options:");
    println!("  --json    Print machine-readable JSON (default output format).");
    println!("  -h, --help");
}

fn parse_args() -> Result<()> {
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--json" => {}
            other => {
                return Err(anyhow!("unknown argument `{}` (use --help)", other));
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    parse_args()?;

    let keys = Keys::generate();
    let npub = keys
        .public_key()
        .to_bech32()
        .with_context(|| "failed encoding generated public key as npub")?;
    let nsec = keys
        .secret_key()
        .to_bech32()
        .with_context(|| "failed encoding generated secret key as nsec")?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "npub": npub,
            "nsec": nsec,
        }))?
    );
    Ok(())
}
