use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use nostr::nips::nip44;
use nostr::nips::nip46::{
    NostrConnectMessage, NostrConnectRequest, NostrConnectResponse, NostrConnectURI,
};
use nostr::{
    ClientMessage, Event, EventBuilder, Filter, JsonUtil, Keys, Kind, PublicKey, RelayMessage,
    SubscriptionId, Timestamp, UnsignedEvent,
};
use tokio::time::{timeout, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Debug, Clone)]
pub struct Nip46SignOutcome {
    pub signed_event: Event,
    pub signer_relay: String,
    pub app_public_key: String,
}

pub async fn sign_event_with_bunker(
    unsigned: &UnsignedEvent,
    bunker_uri: &str,
    app_secret_key: Option<&str>,
    timeout_budget: Duration,
) -> Result<Nip46SignOutcome> {
    let uri =
        NostrConnectURI::parse(bunker_uri).with_context(|| "failed to parse NIP-46 bunker URI")?;
    let (remote_signer_pubkey, relays, bunker_secret) = match uri {
        NostrConnectURI::Bunker {
            remote_signer_public_key,
            relays,
            secret,
        } => (remote_signer_public_key, relays, secret),
        NostrConnectURI::Client { .. } => {
            return Err(anyhow!(
                "NOSTR_NIP46_BUNKER_URI must use bunker:// scheme for remote signer mode"
            ));
        }
    };
    if relays.is_empty() {
        return Err(anyhow!(
            "NIP-46 bunker URI does not include relay endpoints"
        ));
    }

    let app_keys = match app_secret_key.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }) {
        Some(secret) => {
            Keys::parse(secret).with_context(|| "failed to parse NOSTR_NIP46_CLIENT_SECRET_KEY")?
        }
        None => Keys::generate(),
    };

    let mut errors = Vec::new();
    for relay in relays {
        let relay_url = relay.to_string();
        match sign_event_with_relay(
            unsigned,
            &relay_url,
            &remote_signer_pubkey,
            bunker_secret.as_deref(),
            &app_keys,
            timeout_budget,
        )
        .await
        {
            Ok(signed_event) => {
                return Ok(Nip46SignOutcome {
                    signed_event,
                    signer_relay: relay_url,
                    app_public_key: app_keys.public_key().to_hex(),
                });
            }
            Err(error) => errors.push(format!("{}: {:#}", relay_url, error)),
        }
    }

    Err(anyhow!(
        "all NIP-46 relays failed for signing: {}",
        errors.join(" | ")
    ))
}

async fn sign_event_with_relay(
    unsigned: &UnsignedEvent,
    relay: &str,
    remote_signer_pubkey: &PublicKey,
    bunker_secret: Option<&str>,
    app_keys: &Keys,
    timeout_budget: Duration,
) -> Result<Event> {
    let (mut stream, _) = timeout(timeout_budget, connect_async(relay))
        .await
        .with_context(|| format!("timeout connecting NIP-46 relay {}", relay))?
        .with_context(|| format!("failed connecting NIP-46 relay {}", relay))?;

    let subscription_id = SubscriptionId::generate();
    let response_filter = Filter::new()
        .author(*remote_signer_pubkey)
        .kind(Kind::NostrConnect)
        .pubkey(app_keys.public_key())
        .since(Timestamp::now())
        .limit(20);

    send_client_message(
        &mut stream,
        &ClientMessage::req(subscription_id.clone(), vec![response_filter]).as_json(),
        timeout_budget,
        "sending NIP-46 REQ",
    )
    .await?;

    let connect_request = NostrConnectRequest::Connect {
        remote_signer_public_key: app_keys.public_key(),
        secret: bunker_secret.map(ToString::to_string),
    };
    let connect_response = send_request_and_wait_response(
        &mut stream,
        relay,
        remote_signer_pubkey,
        app_keys,
        connect_request,
        timeout_budget,
    )
    .await?;
    let connect_result = connect_response
        .result
        .ok_or_else(|| anyhow!("missing NIP-46 connect result"))?;
    connect_result
        .to_ack()
        .with_context(|| "NIP-46 connect request was not acknowledged")?;

    let sign_request = NostrConnectRequest::SignEvent(unsigned.clone());
    let sign_response = send_request_and_wait_response(
        &mut stream,
        relay,
        remote_signer_pubkey,
        app_keys,
        sign_request,
        timeout_budget,
    )
    .await?;
    let sign_result = sign_response
        .result
        .ok_or_else(|| anyhow!("missing NIP-46 sign_event result"))?;
    let signed_event = sign_result
        .to_sign_event()
        .with_context(|| "invalid NIP-46 sign_event response payload")?;

    let _ = send_client_message(
        &mut stream,
        &ClientMessage::close(subscription_id).as_json(),
        timeout_budget,
        "sending NIP-46 CLOSE",
    )
    .await;
    let _ = stream.close(None).await;

    Ok(signed_event)
}

async fn send_request_and_wait_response(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    relay: &str,
    remote_signer_pubkey: &PublicKey,
    app_keys: &Keys,
    request: NostrConnectRequest,
    timeout_budget: Duration,
) -> Result<NostrConnectResponse> {
    let method = request.method();
    let message = NostrConnectMessage::request(&request);
    let request_id = message.id().to_string();
    let request_event = EventBuilder::nostr_connect(app_keys, *remote_signer_pubkey, message)
        .with_context(|| "failed composing NIP-46 request event")?
        .sign_with_keys(app_keys)
        .with_context(|| "failed signing NIP-46 request event")?;

    send_client_message(
        stream,
        &ClientMessage::event(request_event.clone()).as_json(),
        timeout_budget,
        "sending NIP-46 request EVENT",
    )
    .await?;

    let started = Instant::now();
    loop {
        let elapsed = started.elapsed();
        if elapsed >= timeout_budget {
            return Err(anyhow!(
                "timeout waiting for NIP-46 response from relay {}",
                relay
            ));
        }
        let remaining = timeout_budget - elapsed;

        let frame = timeout(remaining, stream.next())
            .await
            .with_context(|| format!("timeout waiting for NIP-46 relay frame from {}", relay))?;
        let Some(frame) = frame else {
            return Err(anyhow!(
                "relay {} closed connection during NIP-46 request",
                relay
            ));
        };
        let frame = frame.with_context(|| format!("NIP-46 relay frame error from {}", relay))?;
        let Message::Text(text) = frame else {
            continue;
        };

        let relay_message = match RelayMessage::from_json(&text) {
            Ok(message) => message,
            Err(_) => continue,
        };

        match relay_message {
            RelayMessage::Ok {
                event_id,
                status,
                message,
            } => {
                if event_id == request_event.id && !status {
                    return Err(anyhow!(
                        "relay {} rejected NIP-46 request event: {}",
                        relay,
                        message
                    ));
                }
            }
            RelayMessage::Event { event, .. } => {
                if event.kind != Kind::NostrConnect || event.pubkey != *remote_signer_pubkey {
                    continue;
                }
                let plaintext =
                    nip44::decrypt(app_keys.secret_key(), remote_signer_pubkey, &event.content)
                        .with_context(|| "failed decrypting NIP-46 response content")?;
                let response_message = NostrConnectMessage::from_json(plaintext)
                    .with_context(|| "failed decoding NIP-46 response message")?;
                if response_message.id() != request_id {
                    continue;
                }
                let response = response_message
                    .to_response(method)
                    .with_context(|| format!("invalid NIP-46 response for method {}", method))?;
                if let Some(error) = response.error.as_ref() {
                    return Err(anyhow!("NIP-46 signer returned error: {}", error));
                }
                return Ok(response);
            }
            _ => {}
        }
    }
}

async fn send_client_message(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    payload: &str,
    timeout_budget: Duration,
    context: &str,
) -> Result<()> {
    timeout(
        timeout_budget,
        stream.send(Message::Text(payload.to_string())),
    )
    .await
    .with_context(|| format!("timeout {}", context))?
    .with_context(|| format!("failed {}", context))
}

#[cfg(test)]
mod tests {
    use super::sign_event_with_bunker;
    use futures_util::{SinkExt, StreamExt};
    use nostr::nips::nip44;
    use nostr::nips::nip46::{
        NostrConnectMessage, NostrConnectRequest, NostrConnectResponse, ResponseResult,
    };
    use nostr::{ClientMessage, EventBuilder, JsonUtil, Keys, Kind, RelayMessage, SecretKey};
    use std::time::Duration;
    use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

    #[test]
    fn signs_event_via_nip46_bunker() -> Result<(), Box<dyn std::error::Error>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let signer_keys = Keys::new(SecretKey::parse(
                    "3333333333333333333333333333333333333333333333333333333333333333",
                )?);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
                let relay_addr = listener.local_addr()?;
                let bunker_uri = format!(
                    "bunker://{}?relay=ws://{}",
                    signer_keys.public_key().to_hex(),
                    relay_addr
                );
                let signer_keys_for_task = signer_keys.clone();

                tokio::spawn(async move {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    let Ok(mut ws) = accept_async(stream).await else {
                        return;
                    };

                    let mut sub_id = None;
                    while let Some(Ok(Message::Text(text))) = ws.next().await {
                        let Ok(client_msg) = ClientMessage::from_json(&text) else {
                            continue;
                        };
                        match client_msg {
                            ClientMessage::Req {
                                subscription_id, ..
                            } => {
                                sub_id = Some(subscription_id.into_owned());
                            }
                            ClientMessage::Event(event) => {
                                let event = event.into_owned();
                                let ok = RelayMessage::ok(event.id, true, "accepted").as_json();
                                let _ = ws.send(Message::Text(ok)).await;

                                if event.kind == Kind::NostrConnect {
                                    let plaintext = match nip44::decrypt(
                                        signer_keys_for_task.secret_key(),
                                        &event.pubkey,
                                        &event.content,
                                    ) {
                                        Ok(plaintext) => plaintext,
                                        Err(_) => continue,
                                    };
                                    let message = match NostrConnectMessage::from_json(plaintext) {
                                        Ok(message) => message,
                                        Err(_) => continue,
                                    };
                                    let id = message.id().to_string();
                                    let request = match message.to_request() {
                                        Ok(request) => request,
                                        Err(_) => continue,
                                    };

                                    let response = match request {
                                        NostrConnectRequest::Connect { .. } => {
                                            NostrConnectResponse::with_result(ResponseResult::Ack)
                                        }
                                        NostrConnectRequest::SignEvent(unsigned) => {
                                            let signed = match unsigned
                                                .sign_with_keys(&signer_keys_for_task)
                                            {
                                                Ok(event) => event,
                                                Err(_) => continue,
                                            };
                                            NostrConnectResponse::with_result(
                                                ResponseResult::SignEvent(Box::new(signed)),
                                            )
                                        }
                                        _ => NostrConnectResponse::with_error("unsupported"),
                                    };
                                    let response_message =
                                        NostrConnectMessage::response(id, response);
                                    let response_event = match EventBuilder::nostr_connect(
                                        &signer_keys_for_task,
                                        event.pubkey,
                                        response_message,
                                    ) {
                                        Ok(builder) => {
                                            match builder.sign_with_keys(&signer_keys_for_task) {
                                                Ok(event) => event,
                                                Err(_) => continue,
                                            }
                                        }
                                        Err(_) => continue,
                                    };
                                    if let Some(subscription_id) = sub_id.clone() {
                                        let relay_event =
                                            RelayMessage::event(subscription_id, response_event)
                                                .as_json();
                                        let _ = ws.send(Message::Text(relay_event)).await;
                                    }
                                }
                            }
                            ClientMessage::Close(_) => break,
                            _ => {}
                        }
                    }
                });

                let unsigned =
                    EventBuilder::text_note("hello nip46").build(signer_keys.public_key());
                let outcome =
                    sign_event_with_bunker(&unsigned, &bunker_uri, None, Duration::from_secs(2))
                        .await?;

                assert_eq!(outcome.signed_event.kind, Kind::TextNote);
                assert_eq!(outcome.signed_event.pubkey, signer_keys.public_key());
                assert_eq!(outcome.signed_event.content, "hello nip46");
                assert_eq!(outcome.signer_relay, format!("ws://{}", relay_addr));
                Ok(())
            })
    }
}
