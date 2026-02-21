use anyhow::{anyhow, Context, Result};
use nostr::SecretKey;
use serde_json::json;
use std::{collections::HashMap, env, time::Duration};
use worker::nostr_transport::publish_text_note;

#[derive(Debug)]
struct Config {
    relays: Vec<String>,
    destination: String,
    text: String,
    secret_key: String,
    timeout_secs: u64,
}

fn parse_args() -> Result<Config> {
    let mut args = env::args().skip(1);
    let mut relays = Vec::new();
    let mut options: HashMap<String, String> = HashMap::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--relay" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--relay requires a value"))?;
                relays.push(value);
            }
            "--to" | "--text" | "--secret-key" | "--timeout-secs" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("{arg} requires a value"))?;
                options.insert(arg, value);
            }
            other => {
                return Err(anyhow!("unknown argument `{}` (use --help)", other));
            }
        }
    }

    if relays.is_empty() {
        return Err(anyhow!("at least one --relay is required"));
    }

    let destination = options
        .remove("--to")
        .ok_or_else(|| anyhow!("--to <npub|hex> is required"))?;
    let text = options
        .remove("--text")
        .ok_or_else(|| anyhow!("--text is required"))?;

    let secret_key = options
        .remove("--secret-key")
        .or_else(|| env::var("NOSTR_SECRET_KEY").ok())
        .ok_or_else(|| anyhow!("--secret-key is required (or set NOSTR_SECRET_KEY)"))?;

    let timeout_secs = options
        .remove("--timeout-secs")
        .map(|raw| {
            raw.parse::<u64>()
                .with_context(|| format!("invalid --timeout-secs `{raw}`"))
        })
        .transpose()?
        .unwrap_or(10);

    Ok(Config {
        relays,
        destination,
        text,
        secret_key,
        timeout_secs,
    })
}

fn print_help() {
    println!("secureagnt-whitenoise-send");
    println!("Send one White Noise (Nostr text-note + p-tag) message to a target pubkey.");
    println!();
    println!("Usage:");
    println!("  secureagnt-whitenoise-send --relay <ws-url> [--relay <ws-url> ...] \\");
    println!("    --to <npub|hex> --text <message> --secret-key <nsec|hex>");
    println!();
    println!("Options:");
    println!("  --relay <url>          Relay websocket URL (repeatable).");
    println!("  --to <npub|hex>        Recipient pubkey.");
    println!("  --text <message>       Message content.");
    println!("  --secret-key <value>   Sender secret key (or set NOSTR_SECRET_KEY).");
    println!("  --timeout-secs <n>     Relay connect/publish timeout (default: 10).");
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = parse_args()?;
    let secret_key = SecretKey::parse(config.secret_key.trim())
        .with_context(|| "failed to parse --secret-key (expected nsec or hex)")?;

    let result = publish_text_note(
        &secret_key,
        config.destination.as_str(),
        config.text.as_str(),
        &config.relays,
        Duration::from_secs(config.timeout_secs.max(1)),
    )
    .await?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "destination": config.destination,
            "text_len": config.text.len(),
            "relays": config.relays,
            "publish_result": result,
        }))?
    );
    Ok(())
}
