use opencli_rs_core::CliError;
use std::process::ExitStatus;

/// Shell metacharacters / operators that we reject in arguments to prevent
/// command injection when spawning external CLIs.
const DANGEROUS_PATTERNS: &[&str] = &["&&", "||", "|", ";", "$(", "`", ">", "<", "\n"];

/// Check whether the given binary is available on the system PATH.
pub fn is_binary_installed(binary: &str) -> bool {
    let cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    std::process::Command::new(cmd)
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Validate that the supplied arguments do not contain shell injection operators.
pub fn validate_args(args: &[String]) -> Result<(), CliError> {
    for arg in args {
        for pattern in DANGEROUS_PATTERNS {
            if arg.contains(pattern) {
                return Err(CliError::Argument {
                    message: format!(
                        "Argument contains disallowed shell operator '{}': {}",
                        pattern, arg
                    ),
                    suggestions: vec![
                        "Shell operators are not allowed in external CLI arguments".to_string(),
                        "If you need piping, run the external CLI directly in your shell"
                            .to_string(),
                    ],
                });
            }
        }
    }
    Ok(())
}

/// Execute an external CLI by spawning it as a child process.
///
/// stdin, stdout, and stderr are inherited so the user can interact with the
/// external tool as if they had called it directly.
pub async fn execute_external_cli(
    name: &str,
    binary: &str,
    args: &[String],
) -> Result<ExitStatus, CliError> {
    validate_args(args)?;

    if !is_binary_installed(binary) {
        return Err(CliError::CommandExecution {
            message: format!(
                "External CLI '{}' not found: binary '{}' is not installed",
                name, binary
            ),
            suggestions: vec![format!(
                "Install '{}' and make sure it is on your PATH",
                binary
            )],
            source: None,
        });
    }

    tracing::info!(cli = name, binary = binary, args = ?args, "Executing external CLI");

    let status = tokio::process::Command::new(binary)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .map_err(|e| CliError::CommandExecution {
            message: format!("Failed to execute '{}': {}", binary, e),
            suggestions: vec![],
            source: Some(Box::new(e)),
        })?;

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_args_clean() {
        let args = vec!["--flag".to_string(), "value".to_string(), "arg".to_string()];
        assert!(validate_args(&args).is_ok());
    }

    #[test]
    fn test_validate_args_rejects_pipe() {
        let args = vec!["foo".to_string(), "| rm -rf /".to_string()];
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_args_rejects_semicolon() {
        let args = vec!["foo; rm -rf /".to_string()];
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_args_rejects_subshell() {
        let args = vec!["$(whoami)".to_string()];
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_args_rejects_backtick() {
        let args = vec!["`whoami`".to_string()];
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_args_rejects_and() {
        let args = vec!["foo && bar".to_string()];
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_is_binary_installed_known() {
        // A command that exists on all platforms
        if cfg!(target_os = "windows") {
            assert!(is_binary_installed("cmd"));
        } else {
            assert!(is_binary_installed("ls"));
        }
    }

    #[test]
    fn test_is_binary_installed_unknown() {
        assert!(!is_binary_installed("definitely_not_a_real_binary_xyz123"));
    }
}
