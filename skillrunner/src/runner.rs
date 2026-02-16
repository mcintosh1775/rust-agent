use crate::protocol::{InvokeRequest, InvokeResult, SkillMessage};
use std::{ffi::OsStr, time::Duration};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time,
};

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl RunnerConfig {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            timeout: Duration::from_secs(5),
            max_output_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillRunner {
    config: RunnerConfig,
}

impl SkillRunner {
    pub fn new(config: RunnerConfig) -> Self {
        Self { config }
    }

    pub async fn invoke(
        &self,
        request: InvokeRequest,
    ) -> Result<SkillRunnerResult, SkillRunnerError> {
        let mut child = Command::new(&self.config.command)
            .args(self.config.args.iter().map(AsRef::<OsStr>::as_ref))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(SkillRunnerError::SpawnFailed)?;

        let mut stdin = child.stdin.take().ok_or(SkillRunnerError::MissingStdin)?;
        let mut stdout = child.stdout.take().ok_or(SkillRunnerError::MissingStdout)?;

        let request_id = request.id.clone();
        let encoded = request
            .into_message()
            .encode_ndjson()
            .map_err(SkillRunnerError::ProtocolEncode)?;

        stdin
            .write_all(&encoded)
            .await
            .map_err(SkillRunnerError::Io)?;
        stdin.shutdown().await.map_err(SkillRunnerError::Io)?;
        drop(stdin);

        let response_bytes = match time::timeout(
            self.config.timeout,
            read_line_capped(&mut stdout, self.config.max_output_bytes),
        )
        .await
        {
            Ok(result) => match result {
                Ok(bytes) => bytes,
                Err(SkillRunnerError::NoOutput) => {
                    let status = child.wait().await.map_err(SkillRunnerError::Io)?;
                    if !status.success() {
                        return Err(SkillRunnerError::SkillExitedNonZero(status.code()));
                    }
                    return Err(SkillRunnerError::NoOutput);
                }
                Err(error) => {
                    kill_if_alive(&mut child).await;
                    return Err(error);
                }
            },
            Err(_) => {
                kill_if_alive(&mut child).await;
                return Err(SkillRunnerError::Timeout);
            }
        };

        let response = SkillMessage::decode_ndjson(&response_bytes)
            .map_err(SkillRunnerError::ProtocolDecode)?;
        let invoke_result = match response {
            SkillMessage::InvokeResult {
                id,
                output,
                action_requests,
            } => {
                if id != request_id {
                    kill_if_alive(&mut child).await;
                    return Err(SkillRunnerError::MismatchedResponseId {
                        expected: request_id,
                        actual: id,
                    });
                }

                InvokeResult {
                    id,
                    output,
                    action_requests,
                }
            }
            SkillMessage::Error { id, error } => {
                kill_if_alive(&mut child).await;
                return Err(SkillRunnerError::SkillReturnedError {
                    id,
                    code: error.code,
                    message: error.message,
                });
            }
            other => {
                kill_if_alive(&mut child).await;
                return Err(SkillRunnerError::UnexpectedResponse {
                    message_type: message_type_name(&other),
                });
            }
        };

        let wait_status = match time::timeout(self.config.timeout, child.wait()).await {
            Ok(status) => status.map_err(SkillRunnerError::Io)?,
            Err(_) => {
                kill_if_alive(&mut child).await;
                return Err(SkillRunnerError::Timeout);
            }
        };

        if !wait_status.success() {
            return Err(SkillRunnerError::SkillExitedNonZero(wait_status.code()));
        }

        Ok(SkillRunnerResult { invoke_result })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillRunnerResult {
    pub invoke_result: InvokeResult,
}

#[derive(Debug, Error)]
pub enum SkillRunnerError {
    #[error("failed to spawn skill process: {0}")]
    SpawnFailed(std::io::Error),

    #[error("skill process missing stdin")]
    MissingStdin,

    #[error("skill process missing stdout")]
    MissingStdout,

    #[error("i/o error: {0}")]
    Io(std::io::Error),

    #[error("skill process timed out")]
    Timeout,

    #[error("skill output exceeded {max_bytes} bytes")]
    OutputTooLarge { max_bytes: usize },

    #[error("protocol encode failed: {0}")]
    ProtocolEncode(serde_json::Error),

    #[error("protocol decode failed: {0}")]
    ProtocolDecode(serde_json::Error),

    #[error("skill returned mismatched id (expected {expected}, actual {actual})")]
    MismatchedResponseId { expected: String, actual: String },

    #[error("skill returned protocol error {code}: {message} (id={id})")]
    SkillReturnedError {
        id: String,
        code: String,
        message: String,
    },

    #[error("unexpected response message type: {message_type}")]
    UnexpectedResponse { message_type: &'static str },

    #[error("skill exited with non-zero status: {0:?}")]
    SkillExitedNonZero(Option<i32>),

    #[error("skill produced no output before closing stdout")]
    NoOutput,
}

async fn kill_if_alive(child: &mut tokio::process::Child) {
    if let Err(error) = child.kill().await {
        if error.kind() != std::io::ErrorKind::InvalidInput {
            tracing::debug!(%error, "failed to kill child process");
        }
    }
}

async fn read_line_capped<R>(
    reader: &mut R,
    max_output_bytes: usize,
) -> Result<Vec<u8>, SkillRunnerError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(256);
    loop {
        let mut single = [0_u8; 1];
        let read = reader
            .read(&mut single)
            .await
            .map_err(SkillRunnerError::Io)?;
        if read == 0 {
            if bytes.is_empty() {
                return Err(SkillRunnerError::NoOutput);
            }
            return Ok(bytes);
        }

        if bytes.len() + read > max_output_bytes {
            return Err(SkillRunnerError::OutputTooLarge {
                max_bytes: max_output_bytes,
            });
        }
        bytes.extend_from_slice(&single[..read]);
        if single[0] == b'\n' {
            return Ok(bytes);
        }
    }
}

fn message_type_name(message: &SkillMessage) -> &'static str {
    match message {
        SkillMessage::Describe { .. } => "describe",
        SkillMessage::DescribeResult { .. } => "describe_result",
        SkillMessage::Invoke { .. } => "invoke",
        SkillMessage::InvokeResult { .. } => "invoke_result",
        SkillMessage::Error { .. } => "error",
    }
}
