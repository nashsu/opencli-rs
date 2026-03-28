use crate::yaml_parser::parse_yaml_adapter;
use opencli_rs_core::{CliError, Registry};
use std::path::PathBuf;

pub fn user_adapters_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".opencli-rs").join("adapters")
}

pub fn discover_user_adapters(registry: &mut Registry) -> Result<usize, CliError> {
    let dir = user_adapters_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let mut count = 0;
    scan_yaml_dir(&dir, registry, &mut count)?;
    Ok(count)
}

fn scan_yaml_dir(
    dir: &PathBuf,
    registry: &mut Registry,
    count: &mut usize,
) -> Result<(), CliError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            scan_yaml_dir(&path, registry, count)?;
        } else if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
            let content = std::fs::read_to_string(&path)?;
            match parse_yaml_adapter(&content) {
                Ok(cmd) => {
                    tracing::debug!(site = %cmd.site, name = %cmd.name, path = ?path, "Registered user adapter");
                    registry.register(cmd);
                    *count += 1;
                }
                Err(e) => {
                    tracing::warn!(path = ?path, error = %e, "Failed to parse user adapter");
                }
            }
        }
    }
    Ok(())
}
