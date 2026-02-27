use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use nostr::{Event, PublicKey, ToBech32};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::time::Duration;
use tokio::time::{timeout, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct BridgeConfig {
    base_url: String,
    tenant_id: String,
    relay: String,
    agent_pubkey_hex: String,
    operator_allowlist_hex: HashSet<String>,
    trigger_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    recipe_id: String,
    triggered_by_user_id: Option<Uuid>,
    trigger_secret_ref: Option<String>,
    trigger_secret_value: Option<String>,
    auth_proxy_token: Option<String>,
    listen_timeout_secs: u64,
    since_secs: u64,
    max_events: usize,
}

fn print_help() {
    println!("secureagnt-whitenoise-bridge");
    println!("Bridge White Noise relay events into SecureAgnt webhook trigger events.");
    println!();
    println!("Usage:");
    println!("  secureagnt-whitenoise-bridge --relay <ws-url> --agent-pubkey <npub|hex> \\");
    println!("    [--trigger-id <uuid> | --agent-id <uuid>] [--recipe-id <id>] [options]");
    println!();
    println!("Required:");
    println!("  --relay <url>             Relay websocket URL.");
    println!("  --agent-pubkey <npub|hex> Destination pubkey used in #p tag filter.");
    println!();
    println!("Trigger selection:");
    println!("  --trigger-id <uuid>       Existing webhook trigger id to use.");
    println!("  --agent-id <uuid>         Agent id used to create webhook trigger when --trigger-id is omitted.");
    println!();
    println!("Optional:");
    println!("  --base-url <url>          API base URL (default: http://localhost:18080).");
    println!("  --tenant-id <id>          Tenant header value (default: single).");
    println!(
        "  --recipe-id <id>          Recipe id for auto-created trigger (default: operator_chat_v1)."
    );
    println!("  --triggered-by-user-id    Optional user id recorded on auto-created trigger.");
    println!("  --operator-pubkey <key>   Allowlist author pubkey (repeatable).");
    println!("  --trigger-secret-ref <r>  Secret ref used when creating trigger (e.g. env:SECUREAGNT_TRIGGER_SECRET).");
    println!("  --trigger-secret <value>  Value sent as x-trigger-secret when ingesting events.");
    println!("  --auth-proxy-token <v>    Optional x-auth-proxy-token header.");
    println!("  --listen-timeout-secs <n> Listen window in seconds (default: 180).");
    println!("  --since-secs <n>          Relay filter lookback in seconds (default: 300).");
    println!("  --max-events <n>          Stop after N accepted events (default: 1).");
}

fn parse_args() -> Result<BridgeConfig> {
    let mut args = env::args().skip(1);
    let mut options: HashMap<String, String> = HashMap::new();
    let mut operator_allowlist = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--operator-pubkey" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--operator-pubkey requires a value"))?;
                operator_allowlist.push(value);
            }
            "--base-url"
            | "--tenant-id"
            | "--relay"
            | "--agent-pubkey"
            | "--trigger-id"
            | "--agent-id"
            | "--recipe-id"
            | "--triggered-by-user-id"
            | "--trigger-secret-ref"
            | "--trigger-secret"
            | "--auth-proxy-token"
            | "--listen-timeout-secs"
            | "--since-secs"
            | "--max-events" => {
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

    let base_url = options
        .remove("--base-url")
        .unwrap_or_else(|| "http://localhost:18080".to_string());
    let tenant_id = options
        .remove("--tenant-id")
        .unwrap_or_else(|| "single".to_string());
    let relay = options
        .remove("--relay")
        .ok_or_else(|| anyhow!("--relay is required"))?;

    let agent_pubkey_input = options
        .remove("--agent-pubkey")
        .ok_or_else(|| anyhow!("--agent-pubkey is required"))?;
    let agent_pubkey_hex = PublicKey::parse(agent_pubkey_input.trim())
        .with_context(|| "failed parsing --agent-pubkey (expected npub or hex)")?
        .to_hex();

    let trigger_id = options
        .remove("--trigger-id")
        .map(|raw| {
            Uuid::parse_str(raw.trim()).with_context(|| format!("invalid --trigger-id `{raw}`"))
        })
        .transpose()?;
    let agent_id = options
        .remove("--agent-id")
        .map(|raw| {
            Uuid::parse_str(raw.trim()).with_context(|| format!("invalid --agent-id `{raw}`"))
        })
        .transpose()?;
    if trigger_id.is_none() && agent_id.is_none() {
        return Err(anyhow!(
            "provide --trigger-id or --agent-id (for auto-create trigger)"
        ));
    }

    let recipe_id = options
        .remove("--recipe-id")
        .unwrap_or_else(|| "operator_chat_v1".to_string());

    let triggered_by_user_id = options
        .remove("--triggered-by-user-id")
        .map(|raw| {
            Uuid::parse_str(raw.trim())
                .with_context(|| format!("invalid --triggered-by-user-id `{raw}`"))
        })
        .transpose()?;

    let mut operator_allowlist_hex = HashSet::new();
    for raw in operator_allowlist {
        let parsed = PublicKey::parse(raw.trim())
            .with_context(|| format!("failed parsing --operator-pubkey `{raw}`"))?;
        operator_allowlist_hex.insert(parsed.to_hex());
    }

    let listen_timeout_secs = options
        .remove("--listen-timeout-secs")
        .map(|raw| {
            raw.parse::<u64>()
                .with_context(|| format!("invalid --listen-timeout-secs `{raw}`"))
        })
        .transpose()?
        .unwrap_or(180);
    let since_secs = options
        .remove("--since-secs")
        .map(|raw| {
            raw.parse::<u64>()
                .with_context(|| format!("invalid --since-secs `{raw}`"))
        })
        .transpose()?
        .unwrap_or(300);
    let max_events = options
        .remove("--max-events")
        .map(|raw| {
            raw.parse::<usize>()
                .with_context(|| format!("invalid --max-events `{raw}`"))
        })
        .transpose()?
        .unwrap_or(1)
        .max(1);

    Ok(BridgeConfig {
        base_url,
        tenant_id,
        relay,
        agent_pubkey_hex,
        operator_allowlist_hex,
        trigger_id,
        agent_id,
        recipe_id,
        triggered_by_user_id,
        trigger_secret_ref: options.remove("--trigger-secret-ref"),
        trigger_secret_value: options.remove("--trigger-secret"),
        auth_proxy_token: options.remove("--auth-proxy-token"),
        listen_timeout_secs,
        since_secs,
        max_events,
    })
}

fn build_api_headers(config: &BridgeConfig, include_owner_role: bool) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-tenant-id",
        HeaderValue::from_str(config.tenant_id.as_str())?,
    );
    if include_owner_role {
        headers.insert("x-user-role", HeaderValue::from_static("owner"));
    }
    if let Some(user_id) = config.triggered_by_user_id {
        headers.insert(
            "x-user-id",
            HeaderValue::from_str(user_id.to_string().as_str())?,
        );
    }
    if let Some(secret) = config.trigger_secret_value.as_deref().map(str::trim) {
        if !secret.is_empty() {
            headers.insert("x-trigger-secret", HeaderValue::from_str(secret)?);
        }
    }
    if let Some(token) = config.auth_proxy_token.as_deref().map(str::trim) {
        if !token.is_empty() {
            headers.insert("x-auth-proxy-token", HeaderValue::from_str(token)?);
        }
    }
    Ok(headers)
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

async fn ensure_trigger(client: &Client, config: &BridgeConfig) -> Result<Uuid> {
    if let Some(trigger_id) = config.trigger_id {
        return Ok(trigger_id);
    }

    let agent_id = config
        .agent_id
        .ok_or_else(|| anyhow!("--agent-id is required when --trigger-id is omitted"))?;
    let url = endpoint(config.base_url.as_str(), "/v1/triggers/webhook");
    let request_body = json!({
        "agent_id": agent_id,
        "triggered_by_user_id": config.triggered_by_user_id,
        "recipe_id": config.recipe_id,
        "input": {
            "source": "whitenoise.operator",
            "channel": "whitenoise",
            "relay": config.relay,
            "reply_to_event_author": true,
        },
        "requested_capabilities": [],
        "webhook_secret_ref": config.trigger_secret_ref,
    });

    let response = client
        .post(url)
        .headers(build_api_headers(config, true)?)
        .json(&request_body)
        .send()
        .await
        .with_context(|| "failed creating webhook trigger")?;
    let status = response.status();
    let body_text = response
        .text()
        .await
        .with_context(|| "failed reading create trigger response body")?;
    if !status.is_success() {
        return Err(anyhow!(
            "create webhook trigger failed: status={} body={}",
            status,
            body_text
        ));
    }

    let payload: Value =
        serde_json::from_str(&body_text).with_context(|| "invalid create trigger JSON")?;
    let trigger_id = payload
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("create trigger response missing `id`"))?;
    Uuid::parse_str(trigger_id).with_context(|| "invalid trigger id in create response")
}

fn parse_event_frame(message_text: &str, subscription_id: &str) -> Result<Option<Value>> {
    let Ok(parsed) = serde_json::from_str::<Value>(message_text) else {
        return Ok(None);
    };
    let Some(items) = parsed.as_array() else {
        return Ok(None);
    };
    if items.len() < 2 {
        return Ok(None);
    }
    let Some(kind) = items[0].as_str() else {
        return Ok(None);
    };
    if kind != "EVENT" {
        return Ok(None);
    }
    if items.len() < 3 {
        return Err(anyhow!("EVENT frame missing payload"));
    }
    if items[1].as_str() != Some(subscription_id) {
        return Ok(None);
    }
    Ok(Some(items[2].clone()))
}

#[cfg(test)]
mod tests {
    use super::parse_event_frame;

    #[test]
    fn parse_event_frame_extracts_event_payload() {
        let frame = r#"["EVENT","sub-a",{"id":"evt-1","content":"hello"}]"#;
        let parsed = parse_event_frame(frame, "sub-a")
            .expect("frame parse should succeed")
            .expect("event payload should be present");
        assert_eq!(parsed.get("id").and_then(|v| v.as_str()), Some("evt-1"));
    }

    #[test]
    fn parse_event_frame_ignores_non_json_frames() {
        let parsed =
            parse_event_frame("relay notice", "sub-a").expect("non-json should be ignored");
        assert!(parsed.is_none());
    }
}

async fn enqueue_event(
    client: &Client,
    config: &BridgeConfig,
    trigger_id: Uuid,
    relay_event: Value,
    relay: &str,
) -> Result<Value> {
    let event: Event = serde_json::from_value(relay_event.clone())
        .with_context(|| "failed decoding EVENT payload")?;
    let author_hex = event.pubkey.to_hex();
    let author_npub = event
        .pubkey
        .to_bech32()
        .unwrap_or_else(|_| author_hex.clone());
    if !config.operator_allowlist_hex.is_empty()
        && !config.operator_allowlist_hex.contains(&author_hex)
    {
        return Ok(json!({
            "status": "ignored_author_not_allowlisted",
            "event_id": event.id.to_string(),
            "author_pubkey": author_npub,
            "author_pubkey_hex": author_hex,
        }));
    }

    let api_event_id = format!("wn-{}", event.id);
    let payload = json!({
        "channel": "whitenoise",
        "relay": relay,
        "event": relay_event,
        "author_pubkey": author_npub,
        "author_pubkey_hex": author_hex,
    });
    let url = endpoint(
        config.base_url.as_str(),
        format!("/v1/triggers/{trigger_id}/events").as_str(),
    );
    let request_body = json!({
        "event_id": api_event_id,
        "payload": payload,
    });

    let response = client
        .post(url)
        .headers(build_api_headers(config, false)?)
        .json(&request_body)
        .send()
        .await
        .with_context(|| "failed posting trigger event")?;
    let status = response.status();
    let body_text = response
        .text()
        .await
        .with_context(|| "failed reading trigger event response body")?;

    if !status.is_success() {
        return Err(anyhow!(
            "trigger event ingestion failed: status={} body={}",
            status,
            body_text
        ));
    }
    let payload: Value =
        serde_json::from_str(body_text.as_str()).with_context(|| "invalid trigger ingest JSON")?;
    Ok(json!({
        "status": "enqueued",
        "relay_event_id": event.id.to_string(),
        "trigger_event_response": payload,
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = parse_args()?;
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .with_context(|| "failed building HTTP client")?;
    let trigger_id = ensure_trigger(&client, &config).await?;

    let (mut stream, _) = timeout(
        Duration::from_secs(15),
        connect_async(config.relay.as_str()),
    )
    .await
    .with_context(|| format!("timeout connecting relay {}", config.relay))?
    .with_context(|| format!("failed connecting relay {}", config.relay))?;

    let subscription_id = format!("wn_bridge_{}", Uuid::new_v4().simple());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .with_context(|| "system clock error")?
        .as_secs();
    let since = now.saturating_sub(config.since_secs.max(1));
    let req_payload = json!([
        "REQ",
        subscription_id,
        {
            "kinds": [1],
            "#p": [config.agent_pubkey_hex],
            "since": since,
        }
    ])
    .to_string();
    stream
        .send(Message::Text(req_payload))
        .await
        .with_context(|| "failed sending relay REQ subscription")?;

    let started = Instant::now();
    let deadline = started + Duration::from_secs(config.listen_timeout_secs.max(1));
    let mut accepted_count = 0usize;
    let mut results: Vec<Value> = Vec::new();

    while accepted_count < config.max_events {
        if Instant::now() >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = timeout(remaining, stream.next())
            .await
            .with_context(|| "relay read timeout")?;
        let Some(message) = message else {
            break;
        };
        let message = message.with_context(|| "relay stream error")?;
        let Message::Text(text) = message else {
            continue;
        };
        let Some(relay_event) = parse_event_frame(text.as_str(), subscription_id.as_str())? else {
            continue;
        };
        let event_result = enqueue_event(
            &client,
            &config,
            trigger_id,
            relay_event,
            config.relay.as_str(),
        )
        .await?;
        let counted = event_result
            .get("status")
            .and_then(Value::as_str)
            .map(|v| v == "enqueued")
            .unwrap_or(false);
        if counted {
            accepted_count += 1;
        }
        results.push(event_result);
    }

    let close_payload = json!(["CLOSE", subscription_id]).to_string();
    let _ = stream.send(Message::Text(close_payload)).await;
    let _ = stream.close(None).await;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "trigger_id": trigger_id,
            "relay": config.relay,
            "accepted_events": accepted_count,
            "max_events": config.max_events,
            "listen_timeout_secs": config.listen_timeout_secs,
            "results": results,
        }))?
    );
    Ok(())
}
