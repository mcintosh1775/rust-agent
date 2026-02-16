use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackSendResult {
    pub channel: String,
    pub status_code: u16,
    pub response: String,
}

pub async fn send_webhook_message(
    webhook_url: &str,
    channel: &str,
    text: &str,
    timeout: Duration,
) -> Result<SlackSendResult> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .with_context(|| "failed building Slack webhook HTTP client")?;

    let response = client
        .post(webhook_url)
        .json(&json!({
            "text": text,
            "channel": channel,
        }))
        .send()
        .await
        .with_context(|| "failed sending Slack webhook request")?;
    let status = response.status();
    let status_code = status.as_u16();
    let body = response
        .text()
        .await
        .with_context(|| "failed reading Slack webhook response body")?;

    if !status.is_success() {
        return Err(anyhow!(
            "Slack webhook returned HTTP {} body={}",
            status_code,
            body
        ));
    }

    Ok(SlackSendResult {
        channel: channel.to_string(),
        status_code,
        response: body,
    })
}
