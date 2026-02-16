use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use nostr::{EventBuilder, Keys, PublicKey, SecretKey, Tag};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPublishResult {
    pub relay: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrPublishResult {
    pub event_id: String,
    pub accepted_relays: usize,
    pub relay_results: Vec<RelayPublishResult>,
}

pub async fn publish_text_note(
    secret_key: &SecretKey,
    recipient: &str,
    text: &str,
    relays: &[String],
    publish_timeout: Duration,
) -> Result<NostrPublishResult> {
    let recipient_pubkey = PublicKey::parse(recipient)
        .with_context(|| "message.send destination target must be npub/hex for whitenoise")?;
    if relays.is_empty() {
        return Err(anyhow!("Nostr relay list is empty"));
    }

    let keys = Keys::new(secret_key.clone());
    let event = EventBuilder::text_note(text)
        .tag(Tag::public_key(recipient_pubkey))
        .sign_with_keys(&keys)
        .with_context(|| "failed to build/sign Nostr text note event")?;

    let mut relay_results = Vec::with_capacity(relays.len());
    let mut accepted_relays = 0usize;
    for relay in relays {
        let relay_result = publish_event_to_relay(relay, &event, publish_timeout)
            .await
            .with_context(|| format!("relay publish attempt failed for {}", relay));
        match relay_result {
            Ok(result) => {
                if result.ok {
                    accepted_relays += 1;
                }
                relay_results.push(result);
            }
            Err(error) => relay_results.push(RelayPublishResult {
                relay: relay.clone(),
                ok: false,
                message: format!("{error:#}"),
            }),
        }
    }

    if accepted_relays == 0 {
        return Err(anyhow!(
            "Nostr publish failed on all relays for event {}",
            event.id
        ));
    }

    Ok(NostrPublishResult {
        event_id: event.id.to_string(),
        accepted_relays,
        relay_results,
    })
}

async fn publish_event_to_relay(
    relay: &str,
    event: &nostr::Event,
    publish_timeout: Duration,
) -> Result<RelayPublishResult> {
    let (mut stream, _) = timeout(publish_timeout, connect_async(relay))
        .await
        .with_context(|| format!("timeout connecting relay {}", relay))?
        .with_context(|| format!("failed connecting relay {}", relay))?;

    let payload = json!(["EVENT", event]).to_string();
    timeout(publish_timeout, stream.send(Message::Text(payload)))
        .await
        .with_context(|| format!("timeout sending event to relay {}", relay))?
        .with_context(|| format!("failed sending event to relay {}", relay))?;

    let ack = timeout(publish_timeout, stream.next())
        .await
        .with_context(|| format!("timeout waiting ACK from relay {}", relay))?;
    let ack = ack.ok_or_else(|| anyhow!("relay {} closed connection without ACK", relay))?;
    let ack = ack.with_context(|| format!("relay {} ACK stream error", relay))?;

    let (ok, message) = parse_ack_message(ack, event.id.to_string().as_str())?;

    // Best effort close; don't fail ack result if close fails.
    let _ = stream.close(None).await;

    Ok(RelayPublishResult {
        relay: relay.to_string(),
        ok,
        message,
    })
}

fn parse_ack_message(message: Message, event_id: &str) -> Result<(bool, String)> {
    let text = match message {
        Message::Text(text) => text,
        other => return Err(anyhow!("unexpected non-text ACK frame: {:?}", other)),
    };
    let value: Value = serde_json::from_str(&text).with_context(|| "invalid ACK JSON")?;
    let arr = value
        .as_array()
        .ok_or_else(|| anyhow!("ACK message must be array"))?;
    if arr.len() < 4 {
        return Err(anyhow!("ACK message missing expected fields"));
    }
    if arr[0].as_str() != Some("OK") {
        return Err(anyhow!("ACK message is not OK type"));
    }
    let ack_event_id = arr[1]
        .as_str()
        .ok_or_else(|| anyhow!("ACK event id is not string"))?;
    if ack_event_id != event_id {
        return Err(anyhow!("ACK event id mismatch"));
    }
    let ok = arr[2]
        .as_bool()
        .ok_or_else(|| anyhow!("ACK success flag is not bool"))?;
    let message = arr[3]
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| "".to_string());
    Ok((ok, message))
}

#[cfg(test)]
mod tests {
    use super::parse_ack_message;
    use tokio_tungstenite::tungstenite::protocol::Message;

    #[test]
    fn parse_ok_ack() {
        let msg = Message::Text(r#"["OK","abc123",true,"accepted"]"#.to_string());
        let (ok, text) = parse_ack_message(msg, "abc123").expect("ack parse should succeed");
        assert!(ok);
        assert_eq!(text, "accepted");
    }

    #[test]
    fn parse_ack_rejects_mismatch() {
        let msg = Message::Text(r#"["OK","aaa",true,"accepted"]"#.to_string());
        let error = parse_ack_message(msg, "bbb").expect_err("mismatched id must fail");
        assert!(error.to_string().contains("mismatch"));
    }
}
