use std::env;
use std::path::{Path, PathBuf};
use std::{fs, io};

#[test]
fn workspace_versions_in_sync() -> Result<(), Box<dyn std::error::Error>> {
    let repo_root = workspace_root();
    let workspace_version = parse_workspace_version(&repo_root.join("Cargo.toml"))?;

    const TRACKED_MANIFESTS: [&str; 5] = [
        "core/Cargo.toml",
        "api/Cargo.toml",
        "worker/Cargo.toml",
        "skillrunner/Cargo.toml",
        "agntctl/Cargo.toml",
    ];

    let failures = TRACKED_MANIFESTS
        .iter()
        .filter_map(|manifest| {
            let manifest_path = repo_root.join(manifest);
            let crate_version =
                parse_crate_version(&manifest_path, &workspace_version).map_err(|err| {
                    io::Error::new(io::ErrorKind::Other, format!("{manifest} (crate version parse): {err}"))
                })?;

            if crate_version == workspace_version {
                return Ok(None);
            }

            Ok(Some(format!(
                "{manifest}: expected workspace version `{workspace_version}`, found `{crate_version}`"
            )))
        })
        .collect::<Result<Vec<_>, io::Error>>()?;

    assert!(
        failures.is_empty(),
        "Workspace version drift detected:\n{}",
        failures.join("\n")
    );

    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn parse_workspace_version(path: &Path) -> Result<String, String> {
    let manifest = fs::read_to_string(path).map_err(|err| err.to_string())?;

    let mut in_workspace_package = false;
    for raw_line in manifest.lines() {
        let line = raw_line.trim();
        if is_table_header(line) {
            in_workspace_package = line == "[workspace.package]";
            continue;
        }

        if !in_workspace_package || line.starts_with('#') || line.is_empty() {
            continue;
        }

        let (key, value) = parse_key_value(line)?;
        if key == "version" {
            return parse_toml_string(&value).ok_or_else(|| {
                format!("workspace manifest {path:?} has non-string version key value: {value}")
            });
        }
    }

    Err(format!("workspace manifest {path:?} does not define version in [workspace.package]"))
}

fn parse_crate_version(path: &Path, workspace_version: &str) -> Result<String, String> {
    let manifest = fs::read_to_string(path).map_err(|err| err.to_string())?;

    let mut in_package = false;
    let mut resolved_version: Option<String> = None;
    for raw_line in manifest.lines() {
        let line = raw_line.trim();
        if is_table_header(line) {
            in_package = line == "[package]";
            continue;
        }

        if !in_package || line.starts_with('#') || line.is_empty() {
            continue;
        }

        let (key, value) = parse_key_value(line)?;
        if key == "version.workspace" {
            resolved_version = Some(workspace_version.to_string());
            break;
        }

        if key == "version" {
            resolved_version = parse_toml_string(&value);
            break;
        }
    }

    resolved_version
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "crate manifest {path:?} does not define version in [package] (or version is empty)"
            )
        })
}

fn parse_key_value(line: &str) -> Result<(&str, String), String> {
    let (key, rhs) = line
        .split_once('=')
        .ok_or_else(|| format!("invalid key-value line: `{line}`"))?;
    Ok((key.trim(), rhs.trim().to_string()))
}

fn parse_toml_string(raw: &str) -> Option<String> {
    let trimmed = raw.split('#').next().map(str::trim).unwrap_or_default();
    if let Some(inner) = trimmed.strip_prefix('"').and_then(|value| value.strip_suffix('"')) {
        Some(inner.to_string())
    } else {
        None
    }
}

fn is_table_header(line: &str) -> bool {
    line.starts_with('[') && line.ends_with(']')
}
