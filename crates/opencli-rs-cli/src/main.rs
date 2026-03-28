mod args;
mod commands;
mod execution;

use clap::{Arg, ArgAction, Command};
use clap_complete::Shell;
use opencli_rs_core::Registry;
use opencli_rs_discovery::{discover_builtin_adapters, discover_user_adapters};
use opencli_rs_external::{load_external_clis, ExternalCli};
use opencli_rs_output::format::{OutputFormat, RenderOptions};
use opencli_rs_output::render;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use tracing_subscriber::EnvFilter;

use crate::args::coerce_and_validate_args;
use crate::commands::{completion, doctor};
use crate::execution::execute_command;

fn build_cli(registry: &Registry, external_clis: &[ExternalCli]) -> Command {
    let mut app = Command::new("opencli-rs")
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
                .allow_external_subcommands(true),
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
                .arg(
                    Arg::new("goal")
                        .long("goal")
                        .help("Hint for capability naming (e.g. search, hot)"),
                )
                .arg(
                    Arg::new("wait")
                        .long("wait")
                        .default_value("3")
                        .help("Initial wait seconds"),
                )
                .arg(
                    Arg::new("auto")
                        .long("auto")
                        .action(ArgAction::SetTrue)
                        .help(
                        "Enable interactive fuzzing (click buttons/tabs to trigger hidden APIs)",
                    ),
                )
                .arg(Arg::new("click").long("click").help(
                    "Comma-separated labels to click before fuzzing (e.g. 'Comments,CC,字幕')",
                )),
        )
        .subcommand(
            Command::new("cascade")
                .about("Auto-detect authentication strategy for an API endpoint")
                .arg(
                    Arg::new("url")
                        .required(true)
                        .help("API endpoint URL to probe"),
                ),
        )
        .subcommand(
            Command::new("generate")
                .about("One-shot: explore + synthesize + select best adapter")
                .arg(
                    Arg::new("url")
                        .required(true)
                        .help("URL to generate adapter for"),
                )
                .arg(
                    Arg::new("goal")
                        .long("goal")
                        .help("What you want (e.g. hot, search, trending)"),
                )
                .arg(Arg::new("site").long("site").help("Override site name")),
        );

    app
}

fn print_error(err: &opencli_rs_core::CliError) {
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
    // 1. Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
            if std::env::var("OPENCLI_VERBOSE").is_ok() {
                EnvFilter::new("debug")
            } else {
                EnvFilter::new("warn")
            }
        }))
        .init();

    // Check for --daemon flag (used by BrowserBridge to spawn daemon as subprocess)
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--daemon") {
        let port: u16 = std::env::var("OPENCLI_DAEMON_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(19825);
        tracing::info!(port = port, "Starting daemon server");
        match opencli_rs_browser::Daemon::start(port).await {
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
            "explore" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let site = site_matches.get_one::<String>("site").cloned();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let wait: u64 = site_matches
                    .get_one::<String>("wait")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3);
                let auto_fuzz = site_matches.get_flag("auto");
                let click_labels: Vec<String> = site_matches
                    .get_one::<String>("click")
                    .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                    .unwrap_or_default();

                let mut bridge = opencli_rs_browser::BrowserBridge::new(
                    std::env::var("OPENCLI_DAEMON_PORT")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(19825),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let options = opencli_rs_ai::ExploreOptions {
                            timeout: Some(120),
                            max_scrolls: Some(3),
                            capture_network: Some(true),
                            wait_seconds: Some(wait as f64),
                            auto_fuzz: Some(auto_fuzz),
                            click_labels,
                            goal,
                            site_name: site,
                        };
                        let result = opencli_rs_ai::explore(page.as_ref(), url, options).await;
                        let _ = page.close().await;
                        match result {
                            Ok(manifest) => {
                                let output =
                                    serde_json::to_string_pretty(&manifest).unwrap_or_default();
                                println!("{}", output);
                            }
                            Err(e) => {
                                print_error(&e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            "cascade" => {
                let url = site_matches.get_one::<String>("url").unwrap();

                let mut bridge = opencli_rs_browser::BrowserBridge::new(
                    std::env::var("OPENCLI_DAEMON_PORT")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(19825),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let result = opencli_rs_ai::cascade(page.as_ref(), url).await;
                        let _ = page.close().await;
                        match result {
                            Ok(r) => {
                                let output = serde_json::to_string_pretty(&r).unwrap_or_default();
                                println!("{}", output);
                            }
                            Err(e) => {
                                print_error(&e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            "generate" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let mut bridge = opencli_rs_browser::BrowserBridge::new(
                    std::env::var("OPENCLI_DAEMON_PORT")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(19825),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let gen_result = opencli_rs_ai::generate(
                            page.as_ref(),
                            url,
                            goal.as_deref().unwrap_or(""),
                        )
                        .await;
                        let _ = page.close().await;
                        match gen_result {
                            Ok(candidate) => {
                                // Save to ~/.opencli-rs/adapters/{site}/{name}.yaml
                                let home = std::env::var("HOME")
                                    .or_else(|_| std::env::var("USERPROFILE"))
                                    .unwrap_or_else(|_| ".".to_string());
                                let dir = std::path::PathBuf::from(&home)
                                    .join(".opencli-rs")
                                    .join("adapters")
                                    .join(&candidate.site);
                                let _ = std::fs::create_dir_all(&dir);
                                let path = dir.join(format!("{}.yaml", candidate.name));
                                match std::fs::write(&path, &candidate.yaml) {
                                    Ok(_) => {
                                        eprintln!(
                                            "✅ Generated adapter: {} {}",
                                            candidate.site, candidate.name
                                        );
                                        eprintln!(
                                            "   Strategy: {:?}, Confidence: {:.0}%",
                                            candidate.strategy,
                                            candidate.confidence * 100.0
                                        );
                                        eprintln!("   Saved to: {}", path.display());
                                        eprintln!();
                                        eprintln!("   Run it now:");
                                        eprintln!(
                                            "   opencli-rs {} {}",
                                            candidate.site, candidate.name
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!("Generated adapter but failed to save: {}", e);
                                        eprintln!();
                                        println!("{}", candidate.yaml);
                                    }
                                }
                            }
                            Err(e) => {
                                print_error(&e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            _ => {}
        }

        // Check if it's an external CLI
        if let Some(ext) = external_clis.iter().find(|e| e.name == site_name) {
            // Gather remaining args for the external CLI
            let ext_args: Vec<String> = match site_matches.subcommand() {
                Some((sub, sub_matches)) => {
                    let mut args = vec![sub.to_string()];
                    if let Some(rest) = sub_matches.get_many::<std::ffi::OsString>("") {
                        args.extend(rest.map(|s| s.to_string_lossy().to_string()));
                    }
                    args
                }
                None => vec![],
            };

            match opencli_rs_external::execute_external_cli(&ext.name, &ext.binary, &ext_args).await
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
            let _ = app_clone.try_get_matches_from(vec!["opencli-rs", site_name, "--help"]);
        }
    } else {
        // No subcommand specified
        eprintln!("opencli-rs v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("No command specified. Use --help for usage.");
        std::process::exit(1);
    }
}
