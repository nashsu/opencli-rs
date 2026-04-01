# opencli-rs
**[English](README.md) | [中文](README.zh.md) | [日本語](README.ja.md)**

<p align="center">
  <img src="title_screen.png" alt="opencli-rs" width="800" />
</p>

<p align="center">
  <a href="https://autocli.ai"><b>https://autocli.ai</b></a> — AI-powered adapter marketplace & cloud API
</p>

---

Blazing fast, memory-safe command-line tool — **Fetch information from any website with a single command**. Covers Twitter/X, Reddit, YouTube, HackerNews, Bilibili, Zhihu, Xiaohongshu, and [55+ sites](#built-in-commands), with support for controlling Electron desktop apps, integrating local CLI tools (`gh`, `docker`, `kubectl`), powered by browser session reuse and AI-native discovery capabilities.

A **complete rewrite in pure Rust** based on [OpenCLI](https://github.com/jackwener/opencli) (TypeScript). Feature-equivalent, **up to 12x faster**, **10x less memory**, **single 4.7MB binary**, zero runtime dependencies.

**The perfect companion for OpenClaw/Agent** — Give your AI Agent the ability to reach information across the entire web, fetching real-time data from 55+ sites with a single command.
**Built for AI Agents:** Configure `opencli-rs list` in `AGENT.md` or `.cursorrules`, and AI can automatically discover all available tools. Register your local CLI (`opencli-rs register mycli`), and AI can seamlessly invoke all your tools.

**CLI-fy All Desktop Apps!** Turn any Electron app into a command-line tool — Cursor, ChatGPT, Notion, Discord, and more. Reorganize, script, and extend desktop apps; AI can natively control itself — endless possibilities.

## 🚀 Performance Comparison

| Metric | 🦀 opencli-rs (Rust) | 📦 opencli (Node.js) | Improvement |
|------|:-----------------:|:-----------------:|:----:|
| 💾 **Memory Usage (Public Commands)** | 15 MB | 99 MB | **6.6x** |
| 💾 **Memory Usage (Browser Commands)** | 9 MB | 95 MB | **10.6x** |
| 📏 **Binary Size** | 4.7 MB | ~50 MB (node_modules) | **10x** |
| 🔗 **Runtime Dependencies** | None | Node.js 20+ | **Zero deps** |
| ✅ **Test Pass Rate** | 103/122 (84%) | 104/122 (85%) | Near parity |

**⚡ Real-world Command Timing Comparison:**

| Command | 🦀 opencli-rs | 📦 opencli | Speedup |
|------|:----------:|:-------:|:------:|
| `bilibili hot` | **1.66s** | 20.1s | 🔥 **12x** |
| `zhihu hot` | **1.77s** | 20.5s | 🔥 **11.6x** |
| `xueqiu search 茅台` | **1.82s** | 9.2s | ⚡ **5x** |
| `xiaohongshu search` | **5.1s** | 14s | ⚡ **2.7x** |

> Based on automated testing of 122 commands (55 sites), macOS Apple Silicon environment.

## Features

- **55 sites, 333 commands** — Covers Bilibili, Twitter, Reddit, Zhihu, Xiaohongshu, YouTube, Hacker News, and more
- **Browser session reuse** — Reuse logged-in sessions via Chrome extension, no need to manage tokens
- **Declarative YAML Pipeline** — Describe data scraping workflows in YAML, add new adapters with zero code
- **AI-native discovery** — `explore` analyzes website APIs, `generate` auto-creates adapters with one command, `cascade` probes authentication strategies
- **AI-powered generation** — `generate --ai` uses LLM to analyze any website and create working adapters automatically, with cloud sharing via [autocli.ai](https://autocli.ai)
- **Download media & articles** — Download videos (via yt-dlp), articles as Markdown with images localized
- **External CLI passthrough** — Integrate GitHub CLI, Docker, Kubernetes, and other tools
- **Multi-format output** — table, JSON, YAML, CSV, Markdown
- **Single binary** — Compiles to a 4MB static binary with zero runtime dependencies

## Installation
> **Just one file, download and use.** No Node.js, Python, or any runtime needed — just put it in your PATH and go.

### Homebrew (macOS / Linux)

```bash
brew tap nashsu/opencli-rs
brew install opencli-rs
```

### One-line Install Script (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/nashsu/opencli-rs/main/scripts/install.sh | sh
```

Automatically detects your system and architecture, downloads the corresponding binary, and installs to `/usr/local/bin/`.

### Windows (PowerShell)

```powershell
Invoke-WebRequest -Uri "https://github.com/nashsu/opencli-rs/releases/latest/download/opencli-rs-x86_64-pc-windows-msvc.zip" -OutFile opencli-rs.zip
Expand-Archive opencli-rs.zip -DestinationPath .
Move-Item opencli-rs.exe "$env:LOCALAPPDATA\Microsoft\WindowsApps\"
```


### Manual Download (Simplest)

Download the file for your platform from [GitHub Releases](https://github.com/nashsu/opencli-rs/releases/latest):

| Platform | File |
|------|------|
| macOS (Apple Silicon) | `opencli-rs-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `opencli-rs-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `opencli-rs-x86_64-unknown-linux-musl.tar.gz` |
| Linux (ARM64) | `opencli-rs-aarch64-unknown-linux-musl.tar.gz` |
| Windows (x64) | `opencli-rs-x86_64-pc-windows-msvc.zip` |

After extracting, place `opencli-rs` (or `opencli-rs.exe` on Windows) in your system PATH.

### Build from Source

```bash
git clone https://github.com/nashsu/opencli-rs.git
cd opencli-rs
cargo build --release
cp target/release/opencli-rs /usr/local/bin/   # macOS / Linux
```

### Update

Simply re-run the install command or download the latest release to overwrite the existing binary.

### Chrome Extension Setup (required for browser commands)

1. Download `opencli-rs-chrome-extension.zip` from [GitHub Releases](https://github.com/nashsu/opencli-rs/releases/latest)
2. Extract to any directory
3. Open Chrome and go to `chrome://extensions`
4. Enable "Developer mode" (top right toggle)
5. Click "Load unpacked" and select the extracted folder
6. The extension will automatically connect to the opencli-rs daemon

> Public mode commands (hackernews, devto, lobsters, etc.) work without the extension.

## Skill Install

One-click install opencli-rs skill for your AI Agent:

```bash
npx skills add https://github.com/nashsu/opencli-rs-skill
```

## Quick Start

```bash
# View all available commands
opencli-rs --help

# View commands for a specific site
opencli-rs hackernews --help

# Get Hacker News top stories (public API, no browser needed)
opencli-rs hackernews top --limit 10

# JSON format output
opencli-rs hackernews top --limit 5 --format json

# Get Bilibili trending videos (requires browser + Cookie)
opencli-rs bilibili hot --limit 20

# Search Twitter (requires browser + login)
opencli-rs twitter search "rust lang" --limit 10

# Run diagnostics
opencli-rs doctor

# Generate shell completions
opencli-rs completion bash >> ~/.bashrc
opencli-rs completion zsh >> ~/.zshrc
opencli-rs completion fish > ~/.config/fish/completions/opencli-rs.fish
```

## AI Commands

> **Powered by [autocli.ai](https://autocli.ai)** — Get your API token, share adapters with the community, and let AI generate adapters for any website.

### Step 1: Authenticate

```bash
opencli-rs auth
```

This will:
1. Open your browser to [https://autocli.ai/get-token](https://autocli.ai/get-token)
2. Prompt you to enter the token
3. Verify the token with the server
4. Save it to `~/.opencli-rs/config.json`

### Step 2: Generate Adapter with AI

```bash
# AI analyzes the page and generates a working adapter
opencli-rs generate https://www.moltbook.com/ --goal 'list' --ai

# Search for products
opencli-rs generate https://www.amazon.com/ --goal 'search' --ai
```

**How it works:**
1. Searches [autocli.ai](https://autocli.ai) for existing adapters matching the URL
2. If found, shows an interactive list for you to choose:
   ```
   ? Existing adapters found, please select:
   > [exact]   example hot (by alice) - Get trending posts
     [domain]  example search (by bob) - Search articles
     🔄 Regenerate (using AI)
   ```
3. If no match or you choose "Regenerate", AI analyzes the page (DOM structure + API requests) and generates a new YAML adapter
4. The generated adapter is saved locally and uploaded to [autocli.ai](https://autocli.ai) for the community

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `AUTOCLI_API_BASE` | Override server URL | `https://www.autocli.ai` |

## Built-in Commands

Run `opencli-rs --help` to see all available commands.

| Site | Commands | Mode |
|------|------|------|
| **hackernews** | `top` `new` `best` `ask` `show` `jobs` `search` `user` | Public |
| **devto** | `top` `tag` `user` | Public |
| **lobsters** | `hot` `newest` `active` `tag` | Public |
| **stackoverflow** | `hot` `search` `bounties` `unanswered` | Public |
| **steam** | `top-sellers` | Public |
| **linux-do** | `hot` `latest` `search` `categories` `category` `topic` | Public |
| **arxiv** | `search` `paper` | Public |
| **wikipedia** | `search` `summary` `random` `trending` | Public |
| **apple-podcasts** | `search` `episodes` `top` | Public |
| **xiaoyuzhou** | `podcast` `podcast-episodes` `episode` | Public |
| **bbc** | `news` | Public |
| **hf** | `top` | Public |
| **sinafinance** | `news` | Public |
| **google** | `news` `search` `suggest` `trends` | Public / Browser |
| **v2ex** | `hot` `latest` `topic` `node` `user` `member` `replies` `nodes` `daily` `me` `notifications` | Public / Browser |
| **bloomberg** | `main` `markets` `economics` `industries` `tech` `politics` `businessweek` `opinions` `feeds` `news` | Public / Browser |
| **twitter** | `trending` `bookmarks` `profile` `search` `timeline` `thread` `following` `followers` `notifications` `post` `reply` `delete` `like` `article` `follow` `unfollow` `bookmark` `unbookmark` `download` `accept` `reply-dm` `block` `unblock` `hide-reply` | Browser |
| **bilibili** | `hot` `search` `me` `favorite` `history` `feed` `subtitle` `dynamic` `ranking` `following` `user-videos` `download` | Browser |
| **reddit** | `hot` `frontpage` `popular` `search` `subreddit` `read` `user` `user-posts` `user-comments` `upvote` `save` `comment` `subscribe` `saved` `upvoted` | Browser |
| **zhihu** | `hot` `search` `question` `download` | Browser |
| **xiaohongshu** | `search` `notifications` `feed` `user` `download` `publish` `creator-notes` `creator-note-detail` `creator-notes-summary` `creator-profile` `creator-stats` | Browser |
| **xueqiu** | `feed` `hot-stock` `hot` `search` `stock` `watchlist` `earnings-date` | Browser |
| **weibo** | `hot` `search` | Browser |
| **douban** | `search` `top250` `subject` `marks` `reviews` `movie-hot` `book-hot` | Browser |
| **weread** | `shelf` `search` `book` `highlights` `notes` `notebooks` `ranking` | Browser |
| **youtube** | `search` `video` `transcript` | Browser |
| **medium** | `feed` `search` `user` | Browser |
| **substack** | `feed` `search` `publication` | Browser |
| **sinablog** | `hot` `search` `article` `user` | Browser |
| **boss** | `search` `detail` `recommend` `joblist` `greet` `batchgreet` `send` `chatlist` `chatmsg` `invite` `mark` `exchange` `resume` `stats` | Browser |
| **jike** | `feed` `search` `create` `like` `comment` `repost` `notifications` `post` `topic` `user` | Browser |
| **facebook** | `feed` `profile` `search` `friends` `groups` `events` `notifications` `memories` `add-friend` `join-group` | Browser |
| **instagram** | `explore` `profile` `search` `user` `followers` `following` `follow` `unfollow` `like` `unlike` `comment` `save` `unsave` `saved` | Browser |
| **tiktok** | `explore` `search` `profile` `user` `following` `follow` `unfollow` `like` `unlike` `comment` `save` `unsave` `live` `notifications` `friends` | Browser |
| **yollomi** | `generate` `video` `edit` `upload` `models` `remove-bg` `upscale` `face-swap` `restore` `try-on` `background` `object-remover` | Browser |
| **yahoo-finance** | `quote` | Browser |
| **barchart** | `quote` `options` `greeks` `flow` | Browser |
| **linkedin** | `search` | Browser |
| **reuters** | `search` | Browser |
| **smzdm** | `search` | Browser |
| **ctrip** | `search` | Browser |
| **coupang** | `search` `add-to-cart` | Browser |
| **grok** | `ask` | Browser |
| **jimeng** | `generate` `history` | Browser |
| **chaoxing** | `assignments` `exams` | Browser |
| **weixin** | `download` | Browser |
| **doubao** | `status` `new` `send` `read` `ask` | Browser |
| **cursor** | `status` `send` `read` `new` `dump` `composer` `model` `extract-code` `ask` `screenshot` `history` `export` | Desktop |
| **codex** | `status` `send` `read` `new` `dump` `extract-diff` `model` `ask` `screenshot` `history` `export` | Desktop |
| **chatwise** | `status` `new` `send` `read` `ask` `model` `history` `export` `screenshot` | Desktop |
| **chatgpt** | `status` `new` `send` `read` `ask` | Desktop |
| **doubao-app** | `status` `new` `send` `read` `ask` `screenshot` `dump` | Desktop |
| **notion** | `status` `search` `read` `new` `write` `sidebar` `favorites` `export` | Desktop |
| **discord-app** | `status` `send` `read` `channels` `servers` `search` `members` | Desktop |
| **antigravity** | `status` `send` `read` `new` `dump` `extract-code` `model` `watch` | Desktop |

> **Mode legend:** Public = No browser needed, calls API directly; Browser = Requires Chrome + extension; Desktop = Requires the desktop app to be running



## AI Discovery Capabilities

Two approaches to auto-generate adapters:

```bash
# 🤖 AI-powered (recommended): LLM analyzes page and generates adapter
opencli-rs generate https://www.example.com --goal hot --ai
# Searches autocli.ai for existing adapters first, then generates with AI if needed

# 🔧 Rule-based: heuristic analysis without AI
opencli-rs generate https://www.example.com --goal hot

# Explore website API surface (endpoints, framework, stores)
opencli-rs explore https://www.example.com --site mysite

# With interactive fuzzing (click buttons to trigger hidden APIs)
opencli-rs explore https://www.example.com --auto --click "Comments,CC"

# Auto-detect authentication strategy (PUBLIC → COOKIE → HEADER)
opencli-rs cascade https://api.example.com/hot
```

**Discovery features:**
- `.json` suffix probing (Reddit-style REST discovery)
- `__INITIAL_STATE__` extraction (SSR sites like Bilibili, Xiaohongshu)
- Pinia/Vuex store discovery and action mapping
- Auto search endpoint discovery with `--goal search`
- Framework detection (Vue/React/Next.js/Nuxt)

## Download

Download media and articles from supported sites:

```bash
# Download Bilibili video (requires yt-dlp)
opencli-rs bilibili download BV1xxx --output ./videos --quality 1080p

# Download Zhihu article as Markdown with images
opencli-rs zhihu download "https://zhuanlan.zhihu.com/p/xxx" --output ./articles

# Download WeChat article as Markdown with images
opencli-rs weixin download "https://mp.weixin.qq.com/s/xxx" --output ./articles

# Download Twitter/X media (images + videos)
opencli-rs twitter download nash_su --limit 10 --output ./twitter
opencli-rs twitter download --tweet-url "https://x.com/user/status/123" --output ./twitter
```

**Download features:**
- Videos via yt-dlp (cookies extracted from browser automatically, no Keychain prompt)
- Articles as Markdown with YAML frontmatter (title, author, date, source)
- Images downloaded and localized (remote URLs replaced with local `images/img_001.jpg`)
- Output directory structure: `output/article_title/title.md` + `output/article_title/images/`

## External CLI Integration

Integrated external tools (passthrough execution):

| Tool | Description |
|------|------|
| `gh` | GitHub CLI |
| `docker` | Docker CLI |
| `kubectl` | Kubernetes CLI |
| `obsidian` | Obsidian note management |
| `readwise` | Readwise reading management |
| `gws` | Google Workspace CLI |

```bash
# Passthrough to GitHub CLI
opencli-rs gh repo list

# Passthrough to kubectl
opencli-rs kubectl get pods
```

## Output Formats

Switch output format via the `--format` global flag:

```bash
opencli-rs hackernews top --format table    # ASCII table (default)
opencli-rs hackernews top --format json     # JSON
opencli-rs hackernews top --format yaml     # YAML
opencli-rs hackernews top --format csv      # CSV
opencli-rs hackernews top --format md       # Markdown table
```

## Authentication Strategies

Each command uses a different authentication strategy:

| Strategy | Description | Requires Browser |
|------|------|--------------|
| `public` | Public API, no authentication needed | No |
| `cookie` | Requires browser Cookie | Yes |
| `header` | Requires specific request headers | Yes |
| `intercept` | Requires network request interception | Yes |
| `ui` | Requires UI interaction | Yes |

## Custom Adapters

Add custom adapters by creating YAML files under `~/.opencli-rs/adapters/`:

```yaml
# ~/.opencli-rs/adapters/mysite/hot.yaml
site: mysite
name: hot
description: My site hot posts
strategy: public
browser: false

args:
  limit:
    type: int
    default: 20
    description: Number of items

columns: [rank, title, score]

pipeline:
  - fetch: https://api.mysite.com/hot
  - select: data.posts
  - map:
      rank: "${{ index + 1 }}"
      title: "${{ item.title }}"
      score: "${{ item.score }}"
  - limit: "${{ args.limit }}"
```

### Pipeline Steps

| Step | Function | Example |
|------|------|------|
| `fetch` | HTTP request | `fetch: https://api.example.com/data` |
| `evaluate` | Execute JS in browser | `evaluate: "document.title"` |
| `navigate` | Page navigation | `navigate: https://example.com` |
| `click` | Click element | `click: "#button"` |
| `type` | Type text | `type: { selector: "#input", text: "hello" }` |
| `wait` | Wait | `wait: 2000` |
| `select` | Select nested data | `select: data.items` |
| `map` | Data mapping | `map: { title: "${{ item.title }}" }` |
| `filter` | Data filtering | `filter: "item.score > 10"` |
| `sort` | Sort | `sort: { by: score, order: desc }` |
| `limit` | Truncate | `limit: "${{ args.limit }}"` |
| `intercept` | Network interception | `intercept: { pattern: "*/api/*" }` |
| `tap` | State management bridge | `tap: { action: "store.fetch", url: "*/api/*" }` |
| `download` | Download | `download: { type: media }` |

### Template Expressions

Pipelines use the `${{ expression }}` syntax:

```yaml
# Variable access
"${{ args.limit }}"
"${{ item.title }}"
"${{ index + 1 }}"

# Comparison and logic
"${{ item.score > 10 }}"
"${{ item.title && !item.deleted }}"

# Ternary expressions
"${{ item.active ? 'yes' : 'no' }}"

# Pipe filters
"${{ item.title | truncate(30) }}"
"${{ item.tags | join(', ') }}"
"${{ item.name | lower | trim }}"

# String interpolation
"https://api.com/${{ item.id }}.json"

# Fallback
"${{ item.subtitle || 'N/A' }}"

# Math functions
"${{ Math.min(args.limit, 50) }}"
```

**Built-in filters (16):** `default`, `join`, `upper`, `lower`, `trim`, `truncate`, `replace`, `keys`, `length`, `first`, `last`, `json`, `slugify`, `sanitize`, `ext`, `basename`

## Configuration

### Environment Variables

| Variable | Default | Description |
|------|--------|------|
| `OPENCLI_VERBOSE` | - | Enable verbose output |
| `OPENCLI_DAEMON_PORT` | `19825` | Daemon port |
| `OPENCLI_CDP_ENDPOINT` | - | CDP direct endpoint (bypasses Daemon) |
| `OPENCLI_BROWSER_COMMAND_TIMEOUT` | `60` | Command timeout (seconds) |
| `OPENCLI_BROWSER_CONNECT_TIMEOUT` | `30` | Browser connection timeout (seconds) |
| `OPENCLI_BROWSER_EXPLORE_TIMEOUT` | `120` | Explore timeout (seconds) |

### File Paths

| Path | Description |
|------|------|
| `~/.opencli-rs/adapters/` | User custom adapters |
| `~/.opencli-rs/plugins/` | User plugins |
| `~/.opencli-rs/external-clis.yaml` | User external CLI registry |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       User / AI Agent                           │
│                     opencli-rs <site> <command>                  │
└─────────────────────┬───────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                      CLI Layer (clap)                            │
│  main.rs → discovery → clap dynamic subcommands → execution.rs  │
│  ┌───────────┐  ┌───────────────┐  ┌──────────────────┐        │
│  │ Built-in   │  │ Site adapter  │  │ External CLI     │        │
│  │ commands   │  │ commands      │  │ passthrough      │        │
│  │ explore    │  │ bilibili hot  │  │ gh, docker, k8s  │        │
│  │ doctor     │  │ twitter feed  │  │                  │        │
│  └───────────┘  └───────┬───────┘  └──────────────────┘        │
└─────────────────────────┼───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Execution Engine (execution.rs)                │
│             Arg validation → Capability routing → Timeout ctrl  │
│                    ┌─────────┼─────────┐                        │
│                    ▼         ▼         ▼                        │
│              YAML Pipeline  Rust Func  External CLI              │
└────────────────┬────────────────────────────────────────────────┘
                 │
                 ▼
┌──────────────────────────────────────────────────────────────────┐
│  Pipeline Engine                    Browser Bridge                │
│  ┌────────────┐                      ┌─────────────────────┐    │
│  │ fetch      │                      │ BrowserBridge       │    │
│  │ evaluate   │  ──── IPage ────▶    │ DaemonClient (HTTP) │    │
│  │ navigate   │                      │ CdpPage (WebSocket) │    │
│  │ map/filter │                      └──────────┬──────────┘    │
│  │ sort/limit │                                 │               │
│  │ intercept  │                      Daemon (axum:19825)        │
│  │ tap        │                        HTTP + WebSocket          │
│  └────────────┘                                 │               │
│                                                 ▼               │
│  Expression Engine (pest)            Chrome Extension (CDP)      │
│  ${{ expr | filter }}                chrome.debugger API         │
└──────────────────────────────────────────────────────────────────┘
```

### Workspace Structure

```
opencli-rs/
├── crates/
│   ├── opencli-rs-core/        # Core data models: Strategy, CliCommand, Registry, IPage trait, Error
│   ├── opencli-rs-pipeline/    # Pipeline engine: pest expressions, executor, 14 step types
│   ├── opencli-rs-browser/     # Browser bridge: Daemon, DaemonPage, CdpPage, DOM helpers
│   ├── opencli-rs-output/      # Output rendering: table, json, yaml, csv, markdown
│   ├── opencli-rs-discovery/   # Adapter discovery: YAML parsing, build.rs compile-time embedding
│   ├── opencli-rs-external/    # External CLI: loading, detection, passthrough execution
│   ├── opencli-rs-ai/          # AI capabilities: explore, synthesize, cascade, generate
│   └── opencli-rs-cli/         # CLI entry point: clap, execution orchestration, doctor, completion
├── adapters/                   # 333 YAML adapter definitions
│   ├── hackernews/
│   ├── bilibili/
│   ├── twitter/
│   └── ...(55 sites)
└── resources/
    └── external-clis.yaml      # External CLI registry
```

### Improvements over the TypeScript Original

| Improvement | Original (TypeScript) | opencli-rs (Rust) |
|--------|-------------------|-------------------|
| Distribution | Node.js + npm install (~100MB) | Single binary (4.1MB) |
| Startup speed | Read manifest JSON → parse → register | Compile-time embedding, zero file I/O |
| Template engine | JS eval (security risk) | pest PEG parser (type-safe) |
| Concurrent fetch | Non-browser mode pool=5 | FuturesUnordered, concurrency=10 |
| Error system | Single hint string | Structured error chain + multiple suggestions |
| HTTP connections | New fetch each time | reqwest connection pool reuse |
| Memory safety | GC | Ownership system, zero GC pauses |

## Development

```bash
# Build
cargo build

# Test (166 tests)
cargo test --workspace

# Release build (with LTO, ~4MB)
cargo build --release

# Add a new adapter
# 1. Create a YAML file under adapters/<site>/
# 2. Recompile (build.rs auto-embeds)
cargo build
```

## Supported Sites

<details>
<summary>Click to expand all 55 sites</summary>

| Site | Commands | Strategy |
|------|--------|------|
| hackernews | 8 | public |
| bilibili | 12 | cookie |
| twitter | 24 | cookie/intercept |
| reddit | 15 | public/cookie |
| zhihu | 2 | cookie |
| xiaohongshu | 11 | cookie |
| douban | 7 | cookie |
| weibo | 2 | cookie |
| v2ex | 11 | public/cookie |
| bloomberg | 10 | cookie |
| youtube | 4 | cookie |
| wikipedia | 4 | public |
| google | 4 | public/cookie |
| facebook | 10 | cookie |
| instagram | 14 | cookie |
| tiktok | 15 | cookie |
| notion | 8 | ui |
| cursor | 12 | ui |
| chatgpt | 6 | public |
| stackoverflow | 4 | public |
| devto | 3 | public |
| lobsters | 4 | public |
| medium | 3 | cookie |
| substack | 3 | cookie |
| weread | 7 | cookie |
| xueqiu | 7 | cookie |
| boss | 14 | cookie |
| jike | 10 | cookie |
| Other 27 sites | ... | ... |

</details>

## Star History

<a href="https://www.star-history.com/?repos=nashsu%2Fopencli-rs&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&legend=top-left" />
 </picture>
</a>

## License

Apache-2.0
