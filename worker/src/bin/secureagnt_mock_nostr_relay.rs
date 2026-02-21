use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::env;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Clone)]
struct Subscriber {
    conn_id: String,
    subscription_id: String,
    tx: mpsc::UnboundedSender<Message>,
}

fn parse_bind_addr() -> Result<String> {
    let mut args = env::args().skip(1);
    let mut bind_addr = "127.0.0.1:19191".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind_addr = args
                    .next()
                    .ok_or_else(|| anyhow!("--bind requires host:port value"))?;
            }
            "--help" | "-h" => {
                println!("secureagnt-mock-nostr-relay");
                println!("Usage: secureagnt-mock-nostr-relay [--bind 127.0.0.1:19191]");
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown argument `{}` (use --help)", other)),
        }
    }
    Ok(bind_addr)
}

async fn process_connection(
    stream: tokio::net::TcpStream,
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
) -> Result<()> {
    let ws = accept_async(stream)
        .await
        .with_context(|| "websocket handshake failed")?;
    let (mut write, mut read) = ws.split();
    let conn_id = Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let tx_clone = tx.clone();

    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if write.send(message).await.is_err() {
                break;
            }
        }
    });

    while let Some(inbound) = read.next().await {
        let inbound = inbound.with_context(|| "relay read error")?;
        let Message::Text(text) = inbound else {
            continue;
        };
        let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text.as_str()) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        let Some(kind) = items[0].as_str() else {
            continue;
        };
        match kind {
            "REQ" => {
                if items.len() < 2 {
                    continue;
                }
                let Some(subscription_id) = items[1].as_str() else {
                    continue;
                };
                let mut guard = subscribers.lock().await;
                guard.retain(|sub| {
                    !(sub.conn_id == conn_id && sub.subscription_id == subscription_id)
                });
                guard.push(Subscriber {
                    conn_id: conn_id.clone(),
                    subscription_id: subscription_id.to_string(),
                    tx: tx_clone.clone(),
                });
            }
            "CLOSE" => {
                if items.len() < 2 {
                    continue;
                }
                let Some(subscription_id) = items[1].as_str() else {
                    continue;
                };
                let mut guard = subscribers.lock().await;
                guard.retain(|sub| {
                    !(sub.conn_id == conn_id && sub.subscription_id == subscription_id)
                });
            }
            "EVENT" => {
                if items.len() < 2 {
                    continue;
                }
                let event = items[1].clone();
                let event_id = event
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                let ok_frame = json!(["OK", event_id, true, ""]).to_string();
                let _ = tx_clone.send(Message::Text(ok_frame.into()));

                let mut guard = subscribers.lock().await;
                guard.retain(|sub| {
                    let frame = json!(["EVENT", sub.subscription_id, event]).to_string();
                    sub.tx.send(Message::Text(frame.into())).is_ok()
                });
            }
            _ => {}
        }
    }

    {
        let mut guard = subscribers.lock().await;
        guard.retain(|sub| sub.conn_id != conn_id);
    }
    let _ = writer.await;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let bind_addr = parse_bind_addr()?;
    let listener = TcpListener::bind(bind_addr.as_str())
        .await
        .with_context(|| format!("failed binding {}", bind_addr))?;
    eprintln!("mock nostr relay listening on ws://{}", bind_addr);

    let subscribers: Arc<Mutex<Vec<Subscriber>>> = Arc::new(Mutex::new(Vec::new()));
    loop {
        let (stream, _) = listener.accept().await.with_context(|| "accept failed")?;
        let subscribers = subscribers.clone();
        tokio::spawn(async move {
            let _ = process_connection(stream, subscribers).await;
        });
    }
}
