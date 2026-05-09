# AutoCLI Project Summary

AutoCLI is a Rust workspace for a fast command-line tool and browser automation bridge. It provides a large set of site-specific commands for fetching data from websites, browser-based workflows through a Chrome extension/daemon pair, and AI-assisted adapter generation.

## Tech Stack

- Rust 2021 workspace
- Tokio async runtime
- Serde, Serde JSON, Serde YAML
- Reqwest, Axum, Tokio Tungstenite, Tower HTTP
- Clap for CLI parsing
- Vite + TypeScript for the Chrome extension

## Rough Structure

- `crates/autocli-core` shared types and core logic
- `crates/autocli-pipeline` scraping and template pipeline logic
- `crates/autocli-browser` browser/daemon integration
- `crates/autocli-output` formatting and output helpers
- `crates/autocli-discovery` adapter discovery
- `crates/autocli-external` external CLI passthrough
- `crates/autocli-ai` AI adapter generation helpers
- `crates/autocli-cli` main binary
- `extension/` Chrome extension service worker build
- `adapters/`, `assets/`, `docs/`, `prompts/`, `scripts/` supporting content

