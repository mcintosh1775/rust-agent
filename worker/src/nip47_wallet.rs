use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use nostr::nips::nip47::{NostrWalletConnectURI, Request, Response};
use nostr::{ClientMessage, Filter, JsonUtil, Kind, RelayMessage, SubscriptionId, Timestamp};
use tokio::time::{timeout, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Debug, Clone)]
pub struct NwcRequestOutcome {
    pub relay: String,
    pub request_event_id: String,
    pub response_event_id: String,
    pub response: Response,
}

pub async fn send_nwc_request(
    uri_raw: &str,
    request: &Request,
    timeout_budget: Duration,
) -> Result<NwcRequestOutcome> {
    let uri = NostrWalletConnectURI::parse(uri_raw)
        .with_context(|| "failed to parse Nostr Wallet Connect URI")?;
    if uri.relays.is_empty() {
        return Err(anyhow!(
            "Nostr Wallet Connect URI does not include relay endpoints"
        ));
    }

    let mut errors = Vec::new();
    for relay in uri.relays.iter() {
        let relay_url = relay.as_str().to_string();
        match send_nwc_request_to_relay(&uri, request, &relay_url, timeout_budget).await {
            Ok(outcome) => return Ok(outcome),
            Err(error) => errors.push(format!("{}: {:#}", relay_url, error)),
        }
    }

    Err(anyhow!(
        "all NIP-47 relays failed for request: {}",
        errors.join(" | ")
    ))
}

async fn send_nwc_request_to_relay(
    uri: &NostrWalletConnectURI,
    request: &Request,
    relay_url: &str,
    timeout_budget: Duration,
) -> Result<NwcRequestOutcome> {
    let (mut stream, _) = timeout(timeout_budget, connect_async(relay_url))
        .await
        .with_context(|| format!("timeout connecting NIP-47 relay {}", relay_url))?
        .with_context(|| format!("failed connecting NIP-47 relay {}", relay_url))?;

    let request_event = request
        .clone()
        .to_event(uri)
        .with_context(|| "failed building/signing NIP-47 request event")?;

    let subscription_id = SubscriptionId::generate();
    let response_filter = Filter::new()
        .author(uri.public_key)
        .kind(Kind::WalletConnectResponse)
        .pubkey(request_event.pubkey)
        .event(request_event.id)
        .since(Timestamp::now())
        .limit(20);

    send_client_message(
        &mut stream,
        &ClientMessage::req(subscription_id.clone(), vec![response_filter]).as_json(),
        timeout_budget,
        "sending NIP-47 REQ",
    )
    .await?;

    send_client_message(
        &mut stream,
        &ClientMessage::event(request_event.clone()).as_json(),
        timeout_budget,
        "sending NIP-47 request EVENT",
    )
    .await?;

    let started = Instant::now();
    loop {
        let elapsed = started.elapsed();
        if elapsed >= timeout_budget {
            return Err(anyhow!(
                "timeout waiting for NIP-47 response from relay {}",
                relay_url
            ));
        }
        let remaining = timeout_budget - elapsed;
        let frame = timeout(remaining, stream.next()).await.with_context(|| {
            format!("timeout waiting for NIP-47 relay frame from {}", relay_url)
        })?;
        let Some(frame) = frame else {
            return Err(anyhow!(
                "relay {} closed connection during NIP-47 request",
                relay_url
            ));
        };
        let frame =
            frame.with_context(|| format!("NIP-47 relay frame error from {}", relay_url))?;

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
                        "relay {} rejected NIP-47 request event: {}",
                        relay_url,
                        message
                    ));
                }
            }
            RelayMessage::Event { event, .. } => {
                if event.kind != Kind::WalletConnectResponse || event.pubkey != uri.public_key {
                    continue;
                }
                let response = Response::from_event(uri, &event)
                    .with_context(|| "failed parsing/decrypting NIP-47 response event")?;
                let _ = send_client_message(
                    &mut stream,
                    &ClientMessage::close(subscription_id.clone()).as_json(),
                    timeout_budget,
                    "sending NIP-47 CLOSE",
                )
                .await;
                let _ = stream.close(None).await;
                return Ok(NwcRequestOutcome {
                    relay: relay_url.to_string(),
                    request_event_id: request_event.id.to_string(),
                    response_event_id: event.id.to_string(),
                    response,
                });
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
    use super::send_nwc_request;
    use futures_util::{SinkExt, StreamExt};
    use nostr::nips::nip04;
    use nostr::nips::nip47::{
        Method, NIP47Error, PayInvoiceRequest, Request, Response, ResponseResult,
    };
    use nostr::{ClientMessage, EventBuilder, JsonUtil, Keys, Kind, RelayMessage, SecretKey, Tag};
    use std::io::ErrorKind;
    use std::time::Duration;
    use tokio::sync::oneshot;
    use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

    #[test]
    fn roundtrip_pay_invoice_over_nip47_relay() -> Result<(), Box<dyn std::error::Error>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let wallet_keys = Keys::new(SecretKey::parse(
                    "5555555555555555555555555555555555555555555555555555555555555555",
                )?);
                let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
                    Ok(listener) => listener,
                    Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                        // Some constrained CI/sandbox environments disallow local TCP binds.
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                };
                let relay_addr = listener.local_addr()?;
                let app_secret = SecretKey::parse(
                    "6666666666666666666666666666666666666666666666666666666666666666",
                )?;
                let uri = format!(
                    "nostr+walletconnect://{}?secret={}&relay=ws://{}",
                    wallet_keys.public_key().to_hex(),
                    app_secret.to_secret_hex(),
                    relay_addr
                );
                let (tx, rx) = oneshot::channel::<Request>();
                let wallet_keys_for_task = wallet_keys.clone();

                tokio::spawn(async move {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    let Ok(mut ws) = accept_async(stream).await else {
                        return;
                    };
                    let mut sub_id = None;
                    let mut request_sender = Some(tx);

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
                                let ack = RelayMessage::ok(event.id, true, "accepted").as_json();
                                let _ = ws.send(Message::Text(ack)).await;

                                if event.kind != Kind::WalletConnectRequest {
                                    continue;
                                }

                                let decrypted = match nip04::decrypt(
                                    wallet_keys_for_task.secret_key(),
                                    &event.pubkey,
                                    &event.content,
                                ) {
                                    Ok(value) => value,
                                    Err(_) => continue,
                                };
                                let request: Request = match Request::from_json(&decrypted) {
                                    Ok(request) => request,
                                    Err(_) => continue,
                                };
                                if let Some(sender) = request_sender.take() {
                                    let _ = sender.send(request.clone());
                                }
                                let response = Response {
                                    result_type: Method::PayInvoice,
                                    error: None,
                                    result: Some(ResponseResult::PayInvoice(
                                        nostr::nips::nip47::PayInvoiceResponse {
                                            preimage: "mock-preimage-001".to_string(),
                                            fees_paid: Some(12),
                                        },
                                    )),
                                };
                                let encrypted = match nip04::encrypt(
                                    wallet_keys_for_task.secret_key(),
                                    &event.pubkey,
                                    response.as_json(),
                                ) {
                                    Ok(value) => value,
                                    Err(_) => continue,
                                };
                                let response_event =
                                    match EventBuilder::new(Kind::WalletConnectResponse, encrypted)
                                        .tag(Tag::public_key(event.pubkey))
                                        .tag(Tag::event(event.id))
                                        .sign_with_keys(&wallet_keys_for_task)
                                    {
                                        Ok(event) => event,
                                        Err(_) => continue,
                                    };

                                if let Some(subscription_id) = sub_id.clone() {
                                    let relay_event =
                                        RelayMessage::event(subscription_id, response_event)
                                            .as_json();
                                    let _ = ws.send(Message::Text(relay_event)).await;
                                }
                            }
                            ClientMessage::Close(_) => break,
                            _ => {}
                        }
                    }
                });

                let request = Request::pay_invoice(PayInvoiceRequest {
                    id: Some("req-001".to_string()),
                    invoice: "lnbc1mock".to_string(),
                    amount: Some(2100),
                });
                let outcome = send_nwc_request(&uri, &request, Duration::from_secs(2)).await?;
                let relay_request = tokio::time::timeout(Duration::from_secs(2), rx)
                    .await
                    .map_err(|_| "timed out waiting for NIP-47 request capture")?
                    .map_err(|_| "NIP-47 request capture dropped")?;

                assert_eq!(relay_request.method, Method::PayInvoice);
                assert_eq!(relay_request, request);
                assert_eq!(outcome.relay, format!("ws://{}", relay_addr));
                assert!(!outcome.request_event_id.is_empty());
                assert!(!outcome.response_event_id.is_empty());
                let pay_result = outcome.response.to_pay_invoice()?;
                assert_eq!(pay_result.preimage, "mock-preimage-001");
                assert_eq!(pay_result.fees_paid, Some(12));

                Ok(())
            })
    }

    #[test]
    fn surfaces_wallet_error_from_response() -> Result<(), Box<dyn std::error::Error>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let wallet_keys = Keys::new(SecretKey::parse(
                    "7777777777777777777777777777777777777777777777777777777777777777",
                )?);
                let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
                    Ok(listener) => listener,
                    Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                        // Some constrained CI/sandbox environments disallow local TCP binds.
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                };
                let relay_addr = listener.local_addr()?;
                let app_secret = SecretKey::parse(
                    "8888888888888888888888888888888888888888888888888888888888888888",
                )?;
                let uri = format!(
                    "nostr+walletconnect://{}?secret={}&relay=ws://{}",
                    wallet_keys.public_key().to_hex(),
                    app_secret.to_secret_hex(),
                    relay_addr
                );
                let wallet_keys_for_task = wallet_keys.clone();
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
                                let ack = RelayMessage::ok(event.id, true, "accepted").as_json();
                                let _ = ws.send(Message::Text(ack)).await;
                                if event.kind != Kind::WalletConnectRequest {
                                    continue;
                                }
                                let response = Response {
                                    result_type: Method::GetBalance,
                                    error: Some(NIP47Error {
                                        code: nostr::nips::nip47::ErrorCode::QuotaExceeded,
                                        message: "wallet quota exceeded".to_string(),
                                    }),
                                    result: None,
                                };
                                let encrypted = match nip04::encrypt(
                                    wallet_keys_for_task.secret_key(),
                                    &event.pubkey,
                                    response.as_json(),
                                ) {
                                    Ok(value) => value,
                                    Err(_) => continue,
                                };
                                let response_event =
                                    match EventBuilder::new(Kind::WalletConnectResponse, encrypted)
                                        .tag(Tag::public_key(event.pubkey))
                                        .tag(Tag::event(event.id))
                                        .sign_with_keys(&wallet_keys_for_task)
                                    {
                                        Ok(event) => event,
                                        Err(_) => continue,
                                    };
                                if let Some(subscription_id) = sub_id.clone() {
                                    let relay_event =
                                        RelayMessage::event(subscription_id, response_event)
                                            .as_json();
                                    let _ = ws.send(Message::Text(relay_event)).await;
                                }
                            }
                            ClientMessage::Close(_) => break,
                            _ => {}
                        }
                    }
                });

                let outcome =
                    send_nwc_request(&uri, &Request::get_balance(), Duration::from_secs(2)).await?;
                let error = outcome
                    .response
                    .to_get_balance()
                    .expect_err("wallet error should be surfaced");
                assert!(error.to_string().contains("wallet quota exceeded"));
                Ok(())
            })
    }
}
