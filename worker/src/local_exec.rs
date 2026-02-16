use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::timeout};

#[derive(Debug, Clone)]
pub struct LocalExecConfig {
    pub enabled: bool,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub max_memory_bytes: u64,
    pub max_processes: u64,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalExecResult {
    pub template_id: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub async fn execute_local_exec(args: &Value, config: &LocalExecConfig) -> Result<LocalExecResult> {
    if !config.enabled {
        return Err(anyhow!(
            "local.exec is disabled (set WORKER_LOCAL_EXEC_ENABLED=1 to enable)"
        ));
    }

    let template_id = args
        .get("template_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("local.exec args.template_id is required"))?;

    let command_spec = build_command_from_template(template_id, args, config)?;
    let output = run_command(command_spec, config).await?;
    let total_output = output.stdout.len() + output.stderr.len();
    if total_output > config.max_output_bytes {
        return Err(anyhow!(
            "local.exec output exceeded {} bytes",
            config.max_output_bytes
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let Some(exit_code) = output.status.code() else {
        return Err(anyhow!("local.exec terminated by signal"));
    };
    if !output.status.success() {
        return Err(anyhow!(
            "local.exec template `{}` exited non-zero ({}) stderr={}",
            template_id,
            exit_code,
            stderr.trim()
        ));
    }

    Ok(LocalExecResult {
        template_id: template_id.to_string(),
        exit_code,
        stdout,
        stderr,
    })
}

#[derive(Debug, Clone)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
}

fn build_command_from_template(
    template_id: &str,
    args: &Value,
    config: &LocalExecConfig,
) -> Result<CommandSpec> {
    match template_id {
        "file.head" => {
            let path = required_path_arg(args, "path")?;
            let path = validate_read_path(path, &config.read_roots)?;
            let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(20);
            if lines == 0 || lines > 200 {
                return Err(anyhow!(
                    "local.exec file.head lines must be between 1 and 200"
                ));
            }

            Ok(CommandSpec {
                program: "head".to_string(),
                args: vec![
                    "-n".to_string(),
                    lines.to_string(),
                    path.to_string_lossy().to_string(),
                ],
            })
        }
        "file.word_count" => {
            let path = required_path_arg(args, "path")?;
            let path = validate_read_path(path, &config.read_roots)?;
            Ok(CommandSpec {
                program: "wc".to_string(),
                args: vec!["-w".to_string(), path.to_string_lossy().to_string()],
            })
        }
        "file.touch" => {
            let path = required_path_arg(args, "path")?;
            let path = validate_write_path(path, &config.write_roots)?;
            Ok(CommandSpec {
                program: "touch".to_string(),
                args: vec![path.to_string_lossy().to_string()],
            })
        }
        other => Err(anyhow!(
            "unknown local.exec template `{}` (allowed: file.head, file.word_count, file.touch)",
            other
        )),
    }
}

async fn run_command(
    command_spec: CommandSpec,
    config: &LocalExecConfig,
) -> Result<std::process::Output> {
    let mut command = Command::new(&command_spec.program);
    command
        .args(command_spec.args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    apply_unix_limits(&mut command, config.max_memory_bytes, config.max_processes)?;

    let output = timeout(config.timeout, command.output())
        .await
        .with_context(|| "local.exec command timed out")?
        .with_context(|| "failed running local.exec command")?;
    Ok(output)
}

fn required_path_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("local.exec args.{} is required", key))
}

fn validate_read_path(path_raw: &str, roots: &[PathBuf]) -> Result<PathBuf> {
    let path = PathBuf::from(path_raw);
    if !path.is_absolute() {
        return Err(anyhow!(
            "local.exec read path must be absolute: {}",
            path_raw
        ));
    }
    let canonical = fs::canonicalize(&path).with_context(|| {
        format!(
            "failed to resolve local.exec read path `{}`",
            path.display()
        )
    })?;
    if !canonical.is_file() {
        return Err(anyhow!(
            "local.exec read path must resolve to a file: {}",
            canonical.display()
        ));
    }
    if !path_is_within_roots(&canonical, roots) {
        return Err(anyhow!(
            "local.exec read path `{}` is outside allowed roots",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn validate_write_path(path_raw: &str, roots: &[PathBuf]) -> Result<PathBuf> {
    let path = PathBuf::from(path_raw);
    if !path.is_absolute() {
        return Err(anyhow!(
            "local.exec write path must be absolute: {}",
            path_raw
        ));
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("local.exec write path must have a parent: {}", path_raw))?;
    let canonical_parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve parent for local.exec write path `{}`",
            parent.display()
        )
    })?;
    if !path_is_within_roots(&canonical_parent, roots) {
        return Err(anyhow!(
            "local.exec write path parent `{}` is outside allowed roots",
            canonical_parent.display()
        ));
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("local.exec write path missing file name: {}", path_raw))?;
    Ok(canonical_parent.join(file_name))
}

fn path_is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

pub fn parse_roots_from_env(raw: Vec<String>, label: &str) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::with_capacity(raw.len());
    for value in raw {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(anyhow!(
                "{} entry must be absolute path: {}",
                label,
                path.display()
            ));
        }
        let canonical = fs::canonicalize(&path).with_context(|| {
            format!(
                "failed to canonicalize {} entry `{}` (ensure directory exists)",
                label,
                path.display()
            )
        })?;
        if !canonical.is_dir() {
            return Err(anyhow!(
                "{} entry must be a directory: {}",
                label,
                canonical.display()
            ));
        }
        roots.push(canonical);
    }
    Ok(roots)
}

#[cfg(unix)]
fn apply_unix_limits(
    command: &mut Command,
    max_memory_bytes: u64,
    max_processes: u64,
) -> Result<()> {
    use std::io;

    unsafe {
        command.pre_exec(move || {
            if max_memory_bytes > 0 {
                set_rlimit(libc::RLIMIT_AS, max_memory_bytes)?;
            }
            if max_processes > 0 {
                set_rlimit(libc::RLIMIT_NPROC, max_processes)?;
            }
            Ok(())
        });
    }

    fn set_rlimit(resource: u32, limit: u64) -> io::Result<()> {
        let rlim = libc::rlimit {
            rlim_cur: limit as libc::rlim_t,
            rlim_max: limit as libc::rlim_t,
        };
        let rc = unsafe { libc::setrlimit(resource, &rlim) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    Ok(())
}

#[cfg(not(unix))]
fn apply_unix_limits(
    _command: &mut Command,
    _max_memory_bytes: u64,
    _max_processes: u64,
) -> Result<()> {
    Ok(())
}
