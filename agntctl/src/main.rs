use serde::Deserialize;
use std::{env, fs, time::Duration};

const DEFAULT_API_BASE_URL: &str = "http://localhost:3000";
const DEFAULT_TENANT_ID: &str = "single";
const DEFAULT_USER_ROLE: &str = "operator";
const DEFAULT_WINDOW_SECS: u64 = 3600;
const DEFAULT_MAX_QUEUED_RUNS: i64 = 25;
const DEFAULT_MAX_FAILED_RUNS_WINDOW: i64 = 5;
const DEFAULT_MAX_DEAD_LETTER_EVENTS_WINDOW: i64 = 0;
const DEFAULT_MAX_P95_RUN_DURATION_MS: f64 = 5000.0;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = run(args.as_slice());
    std::process::exit(code);
}

fn run(args: &[String]) -> i32 {
    if args.is_empty() || is_help(args[0].as_str()) {
        print_help();
        return 0;
    }
    if is_version(args[0].as_str()) {
        println!("agntctl {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }

    match args[0].as_str() {
        "status" => {
            println!("secureagntd status: unknown (daemon/API wiring pending)");
            0
        }
        "config" => run_config(&args[1..]),
        "skills" => run_skills(&args[1..]),
        "policy" => run_policy(&args[1..]),
        "audit" => run_audit(&args[1..]),
        "ops" => run_ops(&args[1..]),
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            2
        }
    }
}

fn run_config(args: &[String]) -> i32 {
    if matches!(args.first().map(String::as_str), Some("validate")) {
        println!("config validation: ok (schema checks pending)");
        return 0;
    }
    eprintln!("usage: agntctl config validate");
    2
}

fn run_skills(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => {
            println!("skills list: not yet connected");
            0
        }
        Some("info") => {
            if let Some(id) = args.get(1) {
                println!("skills info {id}: not yet connected");
                0
            } else {
                eprintln!("usage: agntctl skills info <id>");
                2
            }
        }
        Some("install") => {
            if let Some(source) = args.get(1) {
                println!("skills install {source}: not yet connected");
                0
            } else {
                eprintln!("usage: agntctl skills install <source>");
                2
            }
        }
        _ => {
            eprintln!("usage: agntctl skills <list|info|install> ...");
            2
        }
    }
}

fn run_policy(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("allow") => {
            println!("policy allow: not yet connected");
            0
        }
        Some("deny") => {
            println!("policy deny: not yet connected");
            0
        }
        _ => {
            eprintln!("usage: agntctl policy <allow|deny> ...");
            2
        }
    }
}

fn run_audit(args: &[String]) -> i32 {
    if matches!(args.first().map(String::as_str), Some("tail")) {
        println!("audit tail: not yet connected");
        return 0;
    }
    eprintln!("usage: agntctl audit tail");
    2
}

fn run_ops(args: &[String]) -> i32 {
    if args.is_empty() || is_help(args[0].as_str()) {
        print_ops_help();
        return 0;
    }

    match args[0].as_str() {
        "soak-gate" => run_ops_soak_gate(&args[1..]),
        other => {
            eprintln!("unknown ops command: {other}");
            print_ops_help();
            2
        }
    }
}

fn run_ops_soak_gate(args: &[String]) -> i32 {
    let mut api_base_url = env::var("AGNTCTL_API_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_BASE_URL.to_string());
    let mut tenant_id = env::var("AGNTCTL_TENANT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TENANT_ID.to_string());
    let mut user_role = env::var("AGNTCTL_USER_ROLE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_USER_ROLE.to_string());
    let mut window_secs = DEFAULT_WINDOW_SECS;
    let mut summary_json_path: Option<String> = None;
    let mut thresholds = OpsSoakThresholds::default();

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => {
                print_ops_soak_gate_help();
                return 0;
            }
            "--api-base-url" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --api-base-url");
                    return 2;
                };
                api_base_url = value.clone();
            }
            "--tenant-id" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --tenant-id");
                    return 2;
                };
                tenant_id = value.clone();
            }
            "--user-role" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --user-role");
                    return 2;
                };
                user_role = value.clone();
            }
            "--window-secs" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --window-secs");
                    return 2;
                };
                match value.parse::<u64>() {
                    Ok(parsed) if parsed > 0 => window_secs = parsed,
                    _ => {
                        eprintln!("invalid --window-secs value: {value}");
                        return 2;
                    }
                }
            }
            "--max-queued-runs" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --max-queued-runs");
                    return 2;
                };
                match value.parse::<i64>() {
                    Ok(parsed) if parsed >= 0 => thresholds.max_queued_runs = parsed,
                    _ => {
                        eprintln!("invalid --max-queued-runs value: {value}");
                        return 2;
                    }
                }
            }
            "--max-failed-runs-window" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --max-failed-runs-window");
                    return 2;
                };
                match value.parse::<i64>() {
                    Ok(parsed) if parsed >= 0 => thresholds.max_failed_runs_window = parsed,
                    _ => {
                        eprintln!("invalid --max-failed-runs-window value: {value}");
                        return 2;
                    }
                }
            }
            "--max-dead-letter-events-window" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --max-dead-letter-events-window");
                    return 2;
                };
                match value.parse::<i64>() {
                    Ok(parsed) if parsed >= 0 => thresholds.max_dead_letter_events_window = parsed,
                    _ => {
                        eprintln!("invalid --max-dead-letter-events-window value: {value}");
                        return 2;
                    }
                }
            }
            "--max-p95-run-duration-ms" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --max-p95-run-duration-ms");
                    return 2;
                };
                match value.parse::<f64>() {
                    Ok(parsed) if parsed > 0.0 => thresholds.max_p95_run_duration_ms = parsed,
                    _ => {
                        eprintln!("invalid --max-p95-run-duration-ms value: {value}");
                        return 2;
                    }
                }
            }
            "--max-avg-run-duration-ms" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --max-avg-run-duration-ms");
                    return 2;
                };
                match value.parse::<f64>() {
                    Ok(parsed) if parsed > 0.0 => {
                        thresholds.max_avg_run_duration_ms = Some(parsed);
                    }
                    _ => {
                        eprintln!("invalid --max-avg-run-duration-ms value: {value}");
                        return 2;
                    }
                }
            }
            "--require-duration-metrics" => {
                thresholds.require_duration_metrics = true;
            }
            "--summary-json" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("missing value for --summary-json");
                    return 2;
                };
                summary_json_path = Some(value.clone());
            }
            other => {
                eprintln!("unknown flag: {other}");
                print_ops_soak_gate_help();
                return 2;
            }
        }

        i += 1;
    }

    let summary = match summary_json_path {
        Some(path) => match read_ops_summary_from_path(path.as_str()) {
            Ok(summary) => summary,
            Err(err) => {
                eprintln!("{err}");
                return 1;
            }
        },
        None => {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    eprintln!("failed creating async runtime: {err}");
                    return 1;
                }
            };

            match runtime.block_on(fetch_ops_summary(
                api_base_url.as_str(),
                tenant_id.as_str(),
                user_role.as_str(),
                window_secs,
            )) {
                Ok(summary) => summary,
                Err(err) => {
                    eprintln!("{err}");
                    return 1;
                }
            }
        }
    };

    let failures = evaluate_ops_summary(&summary, &thresholds);
    println!(
        "ops summary: queued={} running={} succeeded_window={} failed_window={} dead_letter_window={} avg_run_duration_ms={:?} p95_run_duration_ms={:?}",
        summary.queued_runs,
        summary.running_runs,
        summary.succeeded_runs_window,
        summary.failed_runs_window,
        summary.dead_letter_trigger_events_window,
        summary.avg_run_duration_ms,
        summary.p95_run_duration_ms
    );

    if failures.is_empty() {
        println!("soak-gate: pass");
        0
    } else {
        eprintln!("soak-gate: fail");
        for failure in failures {
            eprintln!("- {failure}");
        }
        3
    }
}

fn read_ops_summary_from_path(path: &str) -> Result<OpsSummaryResponse, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed reading summary json `{path}`: {err}"))?;
    serde_json::from_str::<OpsSummaryResponse>(body.as_str())
        .map_err(|err| format!("failed parsing summary json `{path}`: {err}"))
}

async fn fetch_ops_summary(
    api_base_url: &str,
    tenant_id: &str,
    user_role: &str,
    window_secs: u64,
) -> Result<OpsSummaryResponse, String> {
    let trimmed_base = api_base_url.trim_end_matches('/');
    if trimmed_base.is_empty() {
        return Err("api base url must not be empty".to_string());
    }
    if tenant_id.trim().is_empty() {
        return Err("tenant id must not be empty".to_string());
    }
    if user_role.trim().is_empty() {
        return Err("user role must not be empty".to_string());
    }

    let url = format!("{trimmed_base}/v1/ops/summary?window_secs={window_secs}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| format!("failed constructing http client: {err}"))?;

    let response = client
        .get(url.as_str())
        .header("x-tenant-id", tenant_id)
        .header("x-user-role", user_role)
        .send()
        .await
        .map_err(|err| format!("failed requesting ops summary: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed reading response body>".to_string());
        return Err(format!(
            "ops summary request failed: status={status} body={body}"
        ));
    }

    response
        .json::<OpsSummaryResponse>()
        .await
        .map_err(|err| format!("failed decoding ops summary response: {err}"))
}

#[derive(Debug, Clone, Copy)]
struct OpsSoakThresholds {
    max_queued_runs: i64,
    max_failed_runs_window: i64,
    max_dead_letter_events_window: i64,
    max_p95_run_duration_ms: f64,
    max_avg_run_duration_ms: Option<f64>,
    require_duration_metrics: bool,
}

impl Default for OpsSoakThresholds {
    fn default() -> Self {
        Self {
            max_queued_runs: DEFAULT_MAX_QUEUED_RUNS,
            max_failed_runs_window: DEFAULT_MAX_FAILED_RUNS_WINDOW,
            max_dead_letter_events_window: DEFAULT_MAX_DEAD_LETTER_EVENTS_WINDOW,
            max_p95_run_duration_ms: DEFAULT_MAX_P95_RUN_DURATION_MS,
            max_avg_run_duration_ms: None,
            require_duration_metrics: false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpsSummaryResponse {
    queued_runs: i64,
    running_runs: i64,
    succeeded_runs_window: i64,
    failed_runs_window: i64,
    dead_letter_trigger_events_window: i64,
    avg_run_duration_ms: Option<f64>,
    p95_run_duration_ms: Option<f64>,
}

fn evaluate_ops_summary(
    summary: &OpsSummaryResponse,
    thresholds: &OpsSoakThresholds,
) -> Vec<String> {
    let mut failures = Vec::new();

    if summary.queued_runs > thresholds.max_queued_runs {
        failures.push(format!(
            "queued_runs {} exceeds max {}",
            summary.queued_runs, thresholds.max_queued_runs
        ));
    }
    if summary.failed_runs_window > thresholds.max_failed_runs_window {
        failures.push(format!(
            "failed_runs_window {} exceeds max {}",
            summary.failed_runs_window, thresholds.max_failed_runs_window
        ));
    }
    if summary.dead_letter_trigger_events_window > thresholds.max_dead_letter_events_window {
        failures.push(format!(
            "dead_letter_trigger_events_window {} exceeds max {}",
            summary.dead_letter_trigger_events_window, thresholds.max_dead_letter_events_window
        ));
    }
    match summary.p95_run_duration_ms {
        Some(p95) if p95 > thresholds.max_p95_run_duration_ms => failures.push(format!(
            "p95_run_duration_ms {:.2} exceeds max {:.2}",
            p95, thresholds.max_p95_run_duration_ms
        )),
        None if thresholds.require_duration_metrics => {
            failures.push("p95_run_duration_ms is missing but required".to_string());
        }
        _ => {}
    }

    if let Some(max_avg) = thresholds.max_avg_run_duration_ms {
        match summary.avg_run_duration_ms {
            Some(avg) if avg > max_avg => {
                failures.push(format!(
                    "avg_run_duration_ms {:.2} exceeds max {:.2}",
                    avg, max_avg
                ));
            }
            None => {
                failures.push("avg_run_duration_ms is missing for configured threshold".to_string())
            }
            _ => {}
        }
    }

    failures
}

fn is_help(value: &str) -> bool {
    matches!(value, "-h" | "--help" | "help")
}

fn is_version(value: &str) -> bool {
    matches!(value, "-V" | "--version" | "version")
}

fn print_help() {
    println!(
        "agntctl - SecureAgnt control CLI\n\n\
Usage:\n\
  agntctl status\n\
  agntctl config validate\n\
  agntctl skills list\n\
  agntctl skills info <id>\n\
  agntctl skills install <source>\n\
  agntctl policy allow ...\n\
  agntctl policy deny ...\n\
  agntctl audit tail\n\
  agntctl ops soak-gate [flags]\n\
  agntctl --help\n\
  agntctl --version"
    );
}

fn print_ops_help() {
    println!(
        "agntctl ops commands:\n\
  agntctl ops soak-gate [flags]\n\
\n\
Use `agntctl ops soak-gate --help` for gate flags."
    );
}

fn print_ops_soak_gate_help() {
    println!(
        "usage: agntctl ops soak-gate [flags]\n\
\n\
Flags:\n\
  --api-base-url <url>                    API base URL (default http://localhost:3000)\n\
  --tenant-id <tenant>                    Tenant header value (default single)\n\
  --user-role <role>                      Role header value (default operator)\n\
  --window-secs <seconds>                 Rolling window seconds (default 3600)\n\
  --max-queued-runs <count>               Max queued runs threshold (default 25)\n\
  --max-failed-runs-window <count>        Max failed runs threshold (default 5)\n\
  --max-dead-letter-events-window <count> Max dead-letter trigger event threshold (default 0)\n\
  --max-p95-run-duration-ms <ms>          Max p95 run duration threshold (default 5000)\n\
  --max-avg-run-duration-ms <ms>          Optional max average run duration threshold\n\
  --require-duration-metrics              Fail when duration metrics are missing\n\
  --summary-json <path>                   Read summary payload from local JSON file\n\
  --help"
    );
}

#[cfg(test)]
mod tests {
    use super::{evaluate_ops_summary, run, OpsSoakThresholds, OpsSummaryResponse};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn status_command_succeeds() {
        assert_eq!(run(args(&["status"]).as_slice()), 0);
    }

    #[test]
    fn config_validate_succeeds() {
        assert_eq!(run(args(&["config", "validate"]).as_slice()), 0);
    }

    #[test]
    fn unknown_command_fails() {
        assert_eq!(run(args(&["unknown"]).as_slice()), 2);
    }

    #[test]
    fn ops_unknown_flag_fails() {
        assert_eq!(run(args(&["ops", "soak-gate", "--bad-flag"]).as_slice()), 2);
    }

    #[test]
    fn soak_gate_eval_passes_when_within_thresholds() {
        let summary = OpsSummaryResponse {
            queued_runs: 5,
            running_runs: 2,
            succeeded_runs_window: 40,
            failed_runs_window: 1,
            dead_letter_trigger_events_window: 0,
            avg_run_duration_ms: Some(700.0),
            p95_run_duration_ms: Some(1400.0),
        };
        let thresholds = OpsSoakThresholds {
            max_avg_run_duration_ms: Some(1000.0),
            ..OpsSoakThresholds::default()
        };

        assert!(evaluate_ops_summary(&summary, &thresholds).is_empty());
    }

    #[test]
    fn soak_gate_eval_collects_failures() {
        let summary = OpsSummaryResponse {
            queued_runs: 30,
            running_runs: 1,
            succeeded_runs_window: 2,
            failed_runs_window: 7,
            dead_letter_trigger_events_window: 3,
            avg_run_duration_ms: Some(1200.0),
            p95_run_duration_ms: Some(6200.0),
        };
        let thresholds = OpsSoakThresholds {
            max_queued_runs: 20,
            max_failed_runs_window: 3,
            max_dead_letter_events_window: 0,
            max_p95_run_duration_ms: 2000.0,
            max_avg_run_duration_ms: Some(1000.0),
            require_duration_metrics: true,
        };

        let failures = evaluate_ops_summary(&summary, &thresholds);
        assert_eq!(failures.len(), 5);
    }

    #[test]
    fn soak_gate_eval_requires_duration_metrics_when_enabled() {
        let summary = OpsSummaryResponse {
            queued_runs: 0,
            running_runs: 0,
            succeeded_runs_window: 0,
            failed_runs_window: 0,
            dead_letter_trigger_events_window: 0,
            avg_run_duration_ms: None,
            p95_run_duration_ms: None,
        };
        let thresholds = OpsSoakThresholds {
            require_duration_metrics: true,
            ..OpsSoakThresholds::default()
        };

        let failures = evaluate_ops_summary(&summary, &thresholds);
        assert_eq!(failures.len(), 1);
        assert_eq!(
            failures[0],
            "p95_run_duration_ms is missing but required".to_string()
        );
    }
}
