mod args;
mod commands;
mod execution;
mod i18n;

use i18n::t;

use clap::{Arg, ArgAction, Command};
use clap_complete::Shell;
use autocli_core::Registry;
use serde_json::Value;
use autocli_discovery::{discover_builtin_adapters, discover_user_adapters};
use autocli_external::{load_external_clis, ExternalCli};
use autocli_output::format::{OutputFormat, RenderOptions};
use autocli_output::render;
use std::collections::HashMap;
use std::str::FromStr;
use tracing_subscriber::EnvFilter;

use crate::args::coerce_and_validate_args;
use crate::commands::{completion, doctor};
use crate::execution::execute_command;

fn build_cli(registry: &Registry, external_clis: &[ExternalCli]) -> Command {
    let mut app = Command::new("autocli")
        .version(env!("CARGO_PKG_VERSION"))
        .about("AI-driven CLI tool — turns websites into command-line interfaces")
        .arg(
            Arg::new("format")
                .long("format")
                .short('f')
                .global(true)
                .default_value("table")
                .help("Output format: table, json, yaml, csv, md"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Enable verbose output"),
        );

    // Add site subcommands from the adapter registry
    for site in registry.list_sites() {
        let mut site_cmd = Command::new(site.to_string());

        for cmd in registry.list_commands(site) {
            let mut sub = Command::new(cmd.name.clone()).about(cmd.description.clone());

            for arg_def in &cmd.args {
                let mut arg = if arg_def.positional {
                    Arg::new(arg_def.name.clone())
                } else {
                    Arg::new(arg_def.name.clone()).long(arg_def.name.clone())
                };
                if let Some(desc) = &arg_def.description {
                    arg = arg.help(desc.clone());
                }
                if arg_def.required {
                    arg = arg.required(true);
                }
                if let Some(default) = &arg_def.default {
                    // Value::String("x").to_string() produces "\"x\"" (JSON-encoded),
                    // but clap needs the raw string value.
                    let default_str = match default {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    arg = arg.default_value(default_str);
                }
                sub = sub.arg(arg);
            }
            site_cmd = site_cmd.subcommand(sub);
        }
        app = app.subcommand(site_cmd);
    }

    // Add external CLI subcommands
    for ext in external_clis {
        app = app.subcommand(
            Command::new(ext.name.clone())
                .about(ext.description.clone())
                .disable_help_flag(true)
                .disable_version_flag(true)
                .arg(
                    Arg::new("args")
                        .num_args(0..)
                        .trailing_var_arg(true)
                        .allow_hyphen_values(true)
                        .value_parser(clap::builder::OsStringValueParser::new())
                        .help("Arguments passed through to the external CLI"),
                ),
        );
    }

    // Built-in utility subcommands
    app = app
        .subcommand(Command::new("doctor").about("Run diagnostics checks"))
        .subcommand(
            Command::new("completion")
                .about("Generate shell completions")
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(clap::value_parser!(Shell))
                        .help("Target shell: bash, zsh, fish, powershell"),
                ),
        )
        .subcommand(
            Command::new("explore")
                .about("Explore a website's API surface and discover endpoints")
                .arg(Arg::new("url").required(true).help("URL to explore"))
                .arg(Arg::new("site").long("site").help("Override site name"))
                .arg(Arg::new("goal").long("goal").help("Hint for capability naming (e.g. search, hot)"))
                .arg(Arg::new("wait").long("wait").default_value("3").help("Initial wait seconds"))
                .arg(Arg::new("auto").long("auto").action(ArgAction::SetTrue).help("Enable interactive fuzzing (click buttons/tabs to trigger hidden APIs)"))
                .arg(Arg::new("click").long("click").help("Comma-separated labels to click before fuzzing (e.g. 'Comments,CC,字幕')")),
        )
        .subcommand(
            Command::new("cascade")
                .about("Auto-detect authentication strategy for an API endpoint")
                .arg(Arg::new("url").required(true).help("API endpoint URL to probe")),
        )
        .subcommand(
            Command::new("generate")
                .about("One-shot: explore + synthesize + select best adapter")
                .arg(Arg::new("url").required(true).help("URL to generate adapter for"))
                .arg(Arg::new("goal").long("goal").help("What you want (e.g. hot, search, trending)"))
                .arg(Arg::new("site").long("site").help("Override site name"))
                .arg(Arg::new("ai").long("ai").action(ArgAction::SetTrue).help("Use AI (LLM) to analyze and generate adapter (requires ~/.autocli/config.json)")),
        )
        .subcommand(
            Command::new("search")
                .about("Search for existing adapters on autocli.ai")
                .arg(Arg::new("url").required(true).help("URL to search adapters for")),
        )
        .subcommand(
            Command::new("auth")
                .about("Authenticate with AutoCLI"),
        );

    app
}

/// Migrate legacy ~/.opencli-rs directory to ~/.autocli
fn migrate_legacy_config() {
    let home = match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(h) => h,
        Err(_) => return,
    };
    let old_dir = std::path::PathBuf::from(&home).join(".opencli-rs");
    let new_dir = std::path::PathBuf::from(&home).join(".autocli");

    if !old_dir.exists() {
        return;
    }

    // Copy contents to new directory
    if let Err(e) = copy_dir_recursive(&old_dir, &new_dir) {
        eprintln!("{}{}", t("⚠️  配置迁移失败: ", "⚠️  Config migration failed: "), e);
        return;
    }

    // Remove old directory
    if let Err(e) = std::fs::remove_dir_all(&old_dir) {
        eprintln!("{}{}", t("⚠️  无法删除旧配置目录: ", "⚠️  Cannot remove old config dir: "), e);
        return;
    }

    eprintln!("{}", t(
        "✅ 已将配置从 ~/.opencli-rs 迁移到 ~/.autocli",
        "✅ Migrated config from ~/.opencli-rs to ~/.autocli"
    ));
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if !dst_path.exists() {
            // Don't overwrite existing files in new dir
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Kill the process listening on a given port.
fn kill_process_on_port(port: u16) {
    if cfg!(target_os = "windows") {
        // Windows: use netstat to find PID, then taskkill
        let netstat = std::process::Command::new("netstat")
            .args(["-ano"])
            .output();
        if let Ok(output) = netstat {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(&format!(":{}", port)) && line.contains("LISTENING") {
                    if let Some(pid) = line.split_whitespace().last() {
                        let _ = std::process::Command::new("taskkill")
                            .args(["/PID", pid, "/F"])
                            .output();
                        tracing::debug!(port, pid, "Killed old daemon (Windows)");
                        return;
                    }
                }
            }
        }
    } else {
        // macOS/Linux: lsof + kill
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("lsof -ti tcp:{} | xargs kill -9 2>/dev/null", port)])
            .output();
        match output {
            Ok(o) => tracing::debug!(port, stdout = %String::from_utf8_lossy(&o.stdout), "Killed old daemon"),
            Err(e) => tracing::warn!(port, error = %e, "Failed to kill old daemon"),
        }
    }
}

fn save_adapter(site: &str, name: &str, yaml: &str) {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let dir = std::path::PathBuf::from(&home)
        .join(".autocli")
        .join("adapters")
        .join(&site);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.yaml", name));
    match std::fs::write(&path, yaml) {
        Ok(_) => {
            eprintln!("{} {} {}", t("✅ 已生成配置:", "✅ Generated adapter:"), site, name);
            eprintln!("   {}{}", t("保存到: ", "Saved to: "), path.display());
            eprintln!();
            eprintln!("   {}", t("运行命令:", "Run it now:"));
            eprintln!("   autocli {} {}", site, name);
        }
        Err(e) => {
            eprintln!("{}{}", t("生成成功但保存失败: ", "Generated adapter but failed to save: "), e);
            eprintln!();
            println!("{}", yaml);
        }
    }
}

const TOKEN_URL: &str = "https://autocli.ai/get-token";

/// Print token missing message and exit.
fn require_token() -> String {
    let config = autocli_ai::load_config();
    match config.autocli_token {
        Some(t) if !t.is_empty() => t,
        _ => {
            eprintln!("{}", t(
                "❌ 未认证，请先登录获取 Token",
                "❌ Not authenticated. Please login to get your token"
            ));
            eprintln!("   {}", TOKEN_URL);
            eprintln!();
            eprintln!("   {}", t(
                "获取 Token 后运行: autocli auth",
                "After getting your token, run: autocli auth"
            ));
            std::process::exit(1);
        }
    }
}

/// Print token invalid/expired message and exit.
fn token_expired_exit() -> ! {
    eprintln!("{}", t(
        "❌ Token 无效或已过期，请重新获取",
        "❌ Token is invalid or expired. Please get a new one"
    ));
    eprintln!("   {}", TOKEN_URL);
    eprintln!();
    eprintln!("   {}", t(
        "获取新 Token 后运行: autocli auth",
        "After getting a new token, run: autocli auth"
    ));
    std::process::exit(1);
}

/// Adapter match from server search
struct AdapterMatch {
    match_type: String,
    site_name: String,
    cmd_name: String,
    description: String,
    author: String,
    command_uuid: String,
    is_builtin: bool,
}

/// Search server for existing adapter configs matching the URL pattern.
/// Returns Err with message on auth/server failure, Ok with matches on success.
async fn search_existing_adapters(url: &str, token: &str) -> Result<Vec<AdapterMatch>, String> {
    let pattern = autocli_ai::url_to_pattern(url);
    let search_url = autocli_ai::search_url(&pattern);

    eprintln!("{}", t("🔍 搜索已有配置...", "🔍 Searching for existing adapters..."));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let resp = client
        .get(&search_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", autocli_ai::user_agent())
        .send()
        .await
        .map_err(|_| t("❌ 服务器连接失败，请稍后再试", "❌ Server connection failed, please try again later").to_string())?;

    if resp.status().as_u16() == 403 {
        token_expired_exit();
    }
    if !resp.status().is_success() {
        return Err(format!("{}{}", t("❌ 服务器返回错误: ", "❌ Server error: "), resp.status()));
    }

    let body: serde_json::Value = resp.json().await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let matches = body.get("matches")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for m in &matches {
        let match_type = m.get("match_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let site_name = m.get("site").and_then(|s| s.get("name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let cmd_name = m.get("command").and_then(|c| c.get("cmd_name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let description = m.get("command").and_then(|c| c.get("description")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let author = m.get("command").and_then(|c| c.get("author")).and_then(|v| v.as_str())
            .or_else(|| m.get("author").and_then(|v| v.as_str()))
            .unwrap_or("").to_string();
        let command_uuid = m.get("command").and_then(|c| c.get("uuid")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let is_builtin = m.get("command").and_then(|c| c.get("is_builtin")).and_then(|v| v.as_bool()).unwrap_or(false);

        if !command_uuid.is_empty() {
            results.push(AdapterMatch { match_type, site_name, cmd_name, description, author, command_uuid, is_builtin });
        }
    }

    Ok(results)
}

/// Fetch full adapter config by command UUID.
async fn fetch_adapter_config(command_uuid: &str, token: &str) -> Result<String, String> {
    let url = autocli_ai::command_config_url(command_uuid);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", autocli_ai::user_agent())
        .send()
        .await
        .map_err(|_| t("❌ 服务器连接失败，请稍后再试", "❌ Server connection failed, please try again later").to_string())?;

    if resp.status().as_u16() == 403 {
        token_expired_exit();
    }
    if !resp.status().is_success() {
        return Err(format!("{}{}", t("❌ 获取配置失败: ", "❌ Failed to fetch config: "), resp.status()));
    }

    let body: serde_json::Value = resp.json().await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    body.get("config")
        .and_then(|c| c.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| t("❌ 配置内容为空", "❌ Config content is empty").to_string())
}

async fn upload_adapter(yaml: &str) {
    let token = require_token();

    let api_url = autocli_ai::upload_url();

    eprintln!("{}", t("☁️  正在上传配置...", "☁️  Uploading adapter..."));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => { eprintln!("❌ Failed to create HTTP client: {}", e); return; }
    };

    let body = serde_json::json!({ "config": yaml });
    match client
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .header("User-Agent", autocli_ai::user_agent())
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                eprintln!("{}", t("✅ 配置上传成功", "✅ Adapter uploaded successfully"));
            } else if resp.status().as_u16() == 403 {
                token_expired_exit();
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                eprintln!("{}{}: {}", t("❌ 上传失败 ", "❌ Upload failed "), status, &body[..body.len().min(200)]);
            }
        }
        Err(e) => { eprintln!("{}{}", t("❌ 上传失败: ", "❌ Upload failed: "), e); }
    }
}

fn print_error(err: &autocli_core::CliError) {
    eprintln!("{} {}", err.icon(), err);
    let suggestions = err.suggestions();
    if !suggestions.is_empty() {
        eprintln!();
        for s in suggestions {
            eprintln!("  -> {}", s);
        }
    }
}

#[tokio::main]
async fn main() {
    // 0. Migrate from ~/.opencli-rs to ~/.autocli if needed
    migrate_legacy_config();

    // 1. Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
                if std::env::var("AUTOCLI_VERBOSE").is_ok() {
                    EnvFilter::new("debug")
                } else {
                    EnvFilter::new("warn")
                }
            }),
        )
        .init();

    // Check for --daemon flag (used by BrowserBridge to spawn daemon as subprocess)
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--daemon") {
        let port: u16 = std::env::var("AUTOCLI_DAEMON_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(19925);
        tracing::info!(port = port, "Starting daemon server");
        match autocli_browser::Daemon::start(port).await {
            Ok(daemon) => {
                // Wait for shutdown signal (ctrl+c)
                tokio::signal::ctrl_c().await.ok();
                tracing::info!("Shutting down daemon");
                let _ = daemon.shutdown().await;
            }
            Err(e) => {
                eprintln!("Failed to start daemon: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // 1.5. Ensure daemon is running with correct version
    {
        let port: u16 = std::env::var("AUTOCLI_DAEMON_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(19925);
        let current_version = env!("CARGO_PKG_VERSION");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1))
            .build()
            .ok();

        let mut need_start = true;

        if let Some(c) = &client {
            if let Ok(resp) = c.get(&format!("http://127.0.0.1:{}/ping", port)).send().await {
                if resp.status().is_success() {
                    // Daemon is running — check version
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let daemon_version = body.get("version").and_then(|v| v.as_str()).unwrap_or("");
                        if daemon_version == current_version {
                            need_start = false;
                            tracing::debug!(port, version = daemon_version, "Daemon already running with correct version");
                        } else {
                            tracing::info!(daemon_version, current_version, "Daemon version mismatch, restarting");
                            // Kill old daemon by requesting shutdown, or find and kill process on the port
                            kill_process_on_port(port);
                            // Wait briefly for port to free up
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        }

        if need_start {
            if let Ok(exe) = std::env::current_exe() {
                let child = tokio::process::Command::new(exe)
                    .arg("--daemon")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                if let Ok(c) = child {
                    std::mem::forget(c);
                    tracing::debug!(port, version = current_version, "Spawned daemon in background");
                }
            }
        }
    }

    // 1.6. Check for updates (only show for non-format output)
    {
        let format_arg = std::env::args().any(|a| a == "--format" || a == "-f");
        if !format_arg {
            let port: u16 = std::env::var("AUTOCLI_DAEMON_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(19925);
            if let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(1)).build() {
                if let Ok(resp) = client.get(&format!("http://127.0.0.1:{}/check-update", port)).send().await {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        if data.get("update_available").and_then(|v| v.as_bool()).unwrap_or(false) {
                            let latest = data.get("latest_version").and_then(|v| v.as_str()).unwrap_or("");
                            let current = data.get("current_version").and_then(|v| v.as_str()).unwrap_or("");
                            let dl = data.get("download_url").and_then(|v| v.as_str()).unwrap_or("");
                            eprintln!("{}", t(
                                &format!("💡 新版本可用: {} (当前: {}) → {}", latest, current, dl),
                                &format!("💡 Update available: {} (current: {}) → {}", latest, current, dl),
                            ));
                            eprintln!();
                        }
                    }
                }
            }
        }
    }

    // 2. Create registry and discover adapters
    let mut registry = Registry::new();

    match discover_builtin_adapters(&mut registry) {
        Ok(n) => tracing::debug!(count = n, "Discovered builtin adapters"),
        Err(e) => tracing::warn!(error = %e, "Failed to discover builtin adapters"),
    }

    match discover_user_adapters(&mut registry) {
        Ok(n) => tracing::debug!(count = n, "Discovered user adapters"),
        Err(e) => tracing::warn!(error = %e, "Failed to discover user adapters"),
    }

    // 3. Load external CLIs
    let external_clis = match load_external_clis() {
        Ok(clis) => {
            tracing::debug!(count = clis.len(), "Loaded external CLIs");
            clis
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load external CLIs");
            vec![]
        }
    };

    // 4. Build clap app with dynamic subcommands
    let app = build_cli(&registry, &external_clis);
    let matches = app.get_matches();

    let format_str = matches.get_one::<String>("format").unwrap().clone();
    let verbose = matches.get_flag("verbose");

    if verbose {
        tracing::info!("Verbose mode enabled");
    }

    let output_format = OutputFormat::from_str(&format_str).unwrap_or_default();

    // 5. Route: find matching site+command or external CLI
    if let Some((site_name, site_matches)) = matches.subcommand() {
        // Handle built-in utility subcommands
        match site_name {
            "doctor" => {
                doctor::run_doctor().await;
                return;
            }
            "completion" => {
                let shell = site_matches
                    .get_one::<Shell>("shell")
                    .copied()
                    .expect("shell argument required");
                let mut app = build_cli(&registry, &external_clis);
                completion::run_completion(&mut app, shell);
                return;
            }
            "search" => {
                let raw_url = site_matches.get_one::<String>("url").unwrap();
                let url = if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
                    raw_url.clone()
                } else {
                    format!("https://{}", raw_url)
                };
                let token = require_token();

                match search_existing_adapters(&url, &token).await {
                    Ok(matches) if !matches.is_empty() => {
                        let options: Vec<String> = matches.iter().map(|m| {
                            let tag = match m.match_type.as_str() {
                                "exact" => "[exact]  ",
                                "partial" => "[partial]",
                                "domain" => "[domain] ",
                                _ => "[other]  ",
                            };
                            let builtin = if m.is_builtin { " [Built-in]" } else { "" };
                            let desc = if m.description.is_empty() {
                                String::new()
                            } else {
                                format!(" - {}", m.description)
                            };
                            let author = if m.author.is_empty() {
                                String::new()
                            } else {
                                format!(" (by {})", m.author)
                            };
                            format!("{} {} {}{}{}{}", tag, m.site_name, m.cmd_name, builtin, author, desc)
                        }).collect();

                        let selection = inquire::Select::new(
                            t("找到以下配置，请选择:", "Adapters found, please select:"),
                            options,
                        ).prompt();

                        match selection {
                            Ok(chosen) => {
                                let idx = matches.iter().position(|m| {
                                    chosen.contains(&m.cmd_name) && chosen.contains(&m.site_name)
                                });
                                if let Some(i) = idx {
                                    let m = &matches[i];
                                    if m.is_builtin {
                                        eprintln!("{}", t("✅ 这是内建命令，可直接使用:", "✅ This is a built-in command, ready to use:"));
                                        eprintln!("   autocli {} {}", m.site_name, m.cmd_name);
                                    } else {
                                        eprintln!("{}", t("📥 正在下载配置...", "📥 Downloading config..."));
                                        match fetch_adapter_config(&m.command_uuid, &token).await {
                                            Ok(yaml) => {
                                                let yaml_site = yaml.lines()
                                                    .find(|l| l.starts_with("site:"))
                                                    .and_then(|l| l.strip_prefix("site:"))
                                                    .map(|s| s.trim().trim_matches('"').to_string())
                                                    .unwrap_or_else(|| m.site_name.clone());
                                                let yaml_name = yaml.lines()
                                                    .find(|l| l.starts_with("name:"))
                                                    .and_then(|l| l.strip_prefix("name:"))
                                                    .map(|s| s.trim().trim_matches('"').to_string())
                                                    .unwrap_or_else(|| m.cmd_name.clone());
                                                save_adapter(&yaml_site, &yaml_name, &yaml);
                                            }
                                            Err(e) => eprintln!("{}", e),
                                        }
                                    }
                                }
                            }
                            Err(_) => {
                                eprintln!("{}", t("已取消", "Cancelled"));
                            }
                        }
                    }
                    Ok(_) => {
                        eprintln!("{}", t("📭 未找到匹配的配置", "📭 No matching adapters found"));
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            "auth" => {
                // Open browser to get token
                let token_url = "https://autocli.ai/get-token";
                eprintln!("{}", t(
                    "🔑 请在浏览器中获取 Token:",
                    "🔑 Get your token from the browser:"
                ));
                eprintln!("   {}", token_url);
                eprintln!();

                // Open default browser
                let _ = if cfg!(target_os = "macos") {
                    std::process::Command::new("open").arg(token_url).spawn()
                } else if cfg!(target_os = "windows") {
                    std::process::Command::new("cmd").args(["/C", "start", token_url]).spawn()
                } else {
                    std::process::Command::new("xdg-open").arg(token_url).spawn()
                };

                // Token input loop with verification
                loop {
                    let input = inquire::Text::new(t("请输入 Token:", "Enter your token:"))
                        .prompt();

                    let token = match input {
                        Ok(t) => t.trim().to_string(),
                        Err(_) => {
                            eprintln!("{}", t("已取消", "Cancelled"));
                            return;
                        }
                    };

                    if token.is_empty() {
                        eprintln!("{}", t("❌ Token 不能为空", "❌ Token cannot be empty"));
                        continue;
                    }

                    // Verify token with server
                    eprintln!("{}", t("🔍 验证 Token...", "🔍 Verifying token..."));
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .unwrap();

                    let verify_url = "https://autocli.ai/api/auth/tokens/verify";
                    let resp = client
                        .post(verify_url)
                        .header("Content-Type", "application/json")
                        .header("User-Agent", autocli_ai::user_agent())
                        .json(&serde_json::json!({ "token": &token }))
                        .send()
                        .await;

                    match resp {
                        Ok(r) => {
                            let body: serde_json::Value = r.json().await.unwrap_or_default();
                            if body.get("status").and_then(|v| v.as_str()) == Some("valid") {
                                // Save token
                                let mut config = autocli_ai::load_config();
                                config.autocli_token = Some(token);
                                match autocli_ai::save_config(&config) {
                                    Ok(_) => {
                                        eprintln!("{}{}", t("✅ Token 已保存到 ", "✅ Token saved to "), autocli_ai::config::config_path().display());
                                    }
                                    Err(e) => {
                                        eprintln!("{}{}", t("❌ Token 保存失败: ", "❌ Failed to save token: "), e);
                                        std::process::exit(1);
                                    }
                                }
                                break;
                            } else {
                                eprintln!("{}", t("❌ Token 无效，请重新输入", "❌ Invalid token, please try again"));
                                continue;
                            }
                        }
                        Err(_) => {
                            eprintln!("{}", t("❌ 无法连接验证服务器，请检查网络后重试", "❌ Cannot connect to verification server, please check your network and try again"));
                            continue;
                        }
                    }
                }
                return;
            }
            "explore" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let site = site_matches.get_one::<String>("site").cloned();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let wait: u64 = site_matches.get_one::<String>("wait")
                    .and_then(|s| s.parse().ok()).unwrap_or(3);
                let auto_fuzz = site_matches.get_flag("auto");
                let click_labels: Vec<String> = site_matches.get_one::<String>("click")
                    .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                    .unwrap_or_default();

                let mut bridge = autocli_browser::BrowserBridge::new(
                    std::env::var("AUTOCLI_DAEMON_PORT").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(19925),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let options = autocli_ai::ExploreOptions {
                            timeout: Some(120),
                            max_scrolls: Some(3),
                            capture_network: Some(true),
                            wait_seconds: Some(wait as f64),
                            auto_fuzz: Some(auto_fuzz),
                            click_labels,
                            goal,
                            site_name: site,
                        };
                        let result = autocli_ai::explore(page.as_ref(), url, options).await;
                        let _ = page.close().await;
                        match result {
                            Ok(manifest) => {
                                let output = serde_json::to_string_pretty(&manifest).unwrap_or_default();
                                println!("{}", output);
                            }
                            Err(e) => { print_error(&e); std::process::exit(1); }
                        }
                    }
                    Err(e) => { print_error(&e); std::process::exit(1); }
                }
                return;
            }
            "cascade" => {
                let url = site_matches.get_one::<String>("url").unwrap();

                let mut bridge = autocli_browser::BrowserBridge::new(
                    std::env::var("AUTOCLI_DAEMON_PORT").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(19925),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let result = autocli_ai::cascade(page.as_ref(), url).await;
                        let _ = page.close().await;
                        match result {
                            Ok(r) => {
                                let output = serde_json::to_string_pretty(&r).unwrap_or_default();
                                println!("{}", output);
                            }
                            Err(e) => { print_error(&e); std::process::exit(1); }
                        }
                    }
                    Err(e) => { print_error(&e); std::process::exit(1); }
                }
                return;
            }
            "generate" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let _site = site_matches.get_one::<String>("site").cloned();
                let use_ai = site_matches.get_flag("ai");

                let mut bridge = autocli_browser::BrowserBridge::new(
                    std::env::var("AUTOCLI_DAEMON_PORT").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(19925),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        if use_ai {
                            // Require token for --ai
                            let token = require_token();

                            // Step 1: Search server for existing adapters
                            let mut need_ai_generate = false;
                            match search_existing_adapters(url, &token).await {
                                Ok(matches) if !matches.is_empty() => {
                                    // Build TUI selection list
                                    let mut options: Vec<String> = matches.iter().map(|m| {
                                        let tag = match m.match_type.as_str() {
                                            "exact" => "[exact]  ",
                                            "partial" => "[partial]",
                                            "domain" => "[domain] ",
                                            _ => "[other]  ",
                                        };
                                        let builtin = if m.is_builtin { " [Built-in]" } else { "" };
                                        let desc = if m.description.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" - {}", m.description)
                                        };
                                        let author = if m.author.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" (by {})", m.author)
                                        };
                                        format!("{} {} {}{}{}{}", tag, m.site_name, m.cmd_name, builtin, author, desc)
                                    }).collect();
                                    let regenerate_label = t("🔄 重新生成 (使用 AI 分析)", "🔄 Regenerate (using AI)").to_string();
                                    options.push(regenerate_label.clone());

                                    let selection = inquire::Select::new(
                                        t("找到以下已有配置，请选择:", "Existing adapters found, please select:"),
                                        options,
                                    ).prompt();

                                    match selection {
                                        Ok(chosen) => {
                                            if chosen.starts_with("🔄") {
                                                need_ai_generate = true;
                                            } else {
                                                // Find the matching config
                                                let idx = matches.iter().position(|m| {
                                                    chosen.contains(&m.cmd_name) && chosen.contains(&m.site_name)
                                                });
                                                if let Some(i) = idx {
                                                    let m = &matches[i];
                                                    if m.is_builtin {
                                                        eprintln!("{}", t("✅ 这是内建命令，可直接使用:", "✅ This is a built-in command, ready to use:"));
                                                        eprintln!("   autocli {} {}", m.site_name, m.cmd_name);
                                                        let _ = page.close().await;
                                                        return;
                                                    }
                                                    eprintln!("{}", t("📥 正在下载配置...", "📥 Downloading config..."));
                                                    match fetch_adapter_config(&m.command_uuid, &token).await {
                                                        Ok(yaml) => {
                                                            let yaml_site = yaml.lines()
                                                                .find(|l| l.starts_with("site:"))
                                                                .and_then(|l| l.strip_prefix("site:"))
                                                                .map(|s| s.trim().trim_matches('"').to_string())
                                                                .unwrap_or_else(|| m.site_name.clone());
                                                            let yaml_name = yaml.lines()
                                                                .find(|l| l.starts_with("name:"))
                                                                .and_then(|l| l.strip_prefix("name:"))
                                                                .map(|s| s.trim().trim_matches('"').to_string())
                                                                .unwrap_or_else(|| m.cmd_name.clone());
                                                            save_adapter(&yaml_site, &yaml_name, &yaml);
                                                            let _ = page.close().await;
                                                            return;
                                                        }
                                                        Err(e) => {
                                                            eprintln!("{}", e);
                                                            let _ = page.close().await;
                                                            std::process::exit(1);
                                                        }
                                                    }
                                                } else {
                                                    need_ai_generate = true;
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            eprintln!("{}", t("已取消", "Cancelled"));
                                            let _ = page.close().await;
                                            return;
                                        }
                                    }
                                }
                                Ok(_) => {
                                    // No matches found
                                    eprintln!("{}", t("📭 未找到已有配置，开始 AI 生成...", "📭 No existing adapter found, starting AI generation..."));
                                    need_ai_generate = true;
                                }
                                Err(e) => {
                                    eprintln!("{}", e);
                                    let _ = page.close().await;
                                    std::process::exit(1);
                                }
                            }

                            if !need_ai_generate {
                                let _ = page.close().await;
                                return;
                            }

                            // Step 2: AI generation via server API
                            let ai_result = autocli_ai::generate_with_ai(
                                page.as_ref(), url,
                                goal.as_deref().unwrap_or("hot"),
                                &token,
                            ).await;
                            let _ = page.close().await;

                            match ai_result {
                                Ok((site, name, yaml)) => {
                                    save_adapter(&site, &name, &yaml);
                                    upload_adapter(&yaml).await;
                                }
                                Err(e) => { print_error(&e); std::process::exit(1); }
                            }
                        } else {
                            // Rule-based generation (existing flow)
                            let gen_result = autocli_ai::generate(page.as_ref(), url, goal.as_deref().unwrap_or("")).await;
                            let _ = page.close().await;
                            match gen_result {
                                Ok(candidate) => {
                                    save_adapter(&candidate.site, &candidate.name, &candidate.yaml);
                                }
                                Err(e) => { print_error(&e); std::process::exit(1); }
                            }
                        }
                    }
                    Err(e) => { print_error(&e); std::process::exit(1); }
                }
                return;
            }
            _ => {}
        }

        // Check if it's an external CLI
        if let Some(ext) = external_clis.iter().find(|e| e.name == site_name) {
            let ext_args: Vec<String> = site_matches
                .get_many::<std::ffi::OsString>("args")
                .map(|vals| vals.map(|s| s.to_string_lossy().to_string()).collect())
                .unwrap_or_default();

            match autocli_external::execute_external_cli(&ext.name, &ext.binary, &ext_args)
                .await
            {
                Ok(status) => {
                    std::process::exit(status.code().unwrap_or(1));
                }
                Err(e) => {
                    print_error(&e);
                    std::process::exit(1);
                }
            }
        }

        // Check if it's a registered site
        if let Some((cmd_name, cmd_matches)) = site_matches.subcommand() {
            if let Some(cmd) = registry.get(site_name, cmd_name) {
                // Collect raw args from clap matches
                let mut raw_args: HashMap<String, String> = HashMap::new();
                for arg_def in &cmd.args {
                    if let Some(val) = cmd_matches.get_one::<String>(&arg_def.name) {
                        raw_args.insert(arg_def.name.clone(), val.clone());
                    }
                }

                // Coerce and validate
                let kwargs = match coerce_and_validate_args(&cmd.args, &raw_args) {
                    Ok(kw) => kw,
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                };

                let start = std::time::Instant::now();

                match execute_command(cmd, kwargs).await {
                    Ok(data) => {
                        let opts = RenderOptions {
                            format: output_format,
                            columns: if cmd.columns.is_empty() {
                                None
                            } else {
                                Some(cmd.columns.clone())
                            },
                            title: None,
                            elapsed: Some(start.elapsed()),
                            source: Some(cmd.full_name()),
                            footer_extra: None,
                        };
                        let output = render(&data, &opts);
                        println!("{}", output);
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("Unknown command: {} {}", site_name, cmd_name);
                std::process::exit(1);
            }
        } else {
            // Site specified but no command — show site help
            // Re-build and print help for just this site subcommand
            let app = build_cli(&registry, &external_clis);
            let app_clone = app;
            // Try to print subcommand help
            let _ = app_clone.try_get_matches_from(vec!["autocli", site_name, "--help"]);
        }
    } else {
        // No subcommand specified
        eprintln!("autocli v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("No command specified. Use --help for usage.");
        std::process::exit(1);
    }
}
