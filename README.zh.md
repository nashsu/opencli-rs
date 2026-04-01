# opencli-rs

**[English](README.md) | [中文](README.zh.md) | [日本語](README.ja.md)**

<p align="center">
  <img src="title_screen.png" alt="opencli-rs" width="800" />
</p>

<p align="center">
  <a href="https://autocli.ai"><b>https://autocli.ai</b></a> — AI 驱动的配置市场 & 云端 API
</p>

---

极速、安全的命令行工具 —— **一行命令快速获取任意网站信息**。覆盖 Bilibili、知乎、小红书、Twitter/X、Reddit、YouTube、HackerNews 等 [55+ 站点](#内置命令)，同时支持控制 Electron 桌面应用、集成本地 CLI 工具（`gh`、`docker`、`kubectl`），通过浏览器会话复用和 AI 原生发现能力驱动。

基于 [OpenCLI](https://github.com/jackwener/opencli)（TypeScript）用 **纯 Rust 完整重写**。功能对等，**最高快 12 倍**，**内存省 10 倍**，**单文件 4.7MB**，零运行时依赖。

**OpenClaw/Agent 的最佳搭档** —— 赋予你的 AI Agent 触达全网信息的能力，一行命令获取 55+ 站点的实时数据。
**为 AI Agent 而生：** 在 `AGENT.md` 或 `.cursorrules` 中配置 `opencli-rs list`，AI 即可自动发现所有可用工具。注册你的本地 CLI（`opencli-rs register mycli`），AI 就能完美调用你的所有工具。

**CLI 化一切桌面应用！** 将任何 Electron 应用变成命令行工具 —— Cursor、ChatGPT、Notion、Discord 等。重组、脚本化、扩展桌面应用，AI 可以原生控制自身，无限可能。

## 🚀 性能对比

| 指标 | 🦀 opencli-rs (Rust) | 📦 opencli (Node.js) | 提升 |
|------|:-----------------:|:-----------------:|:----:|
| 💾 **内存占用 (Public 命令)** | 15 MB | 99 MB | **6.6x** |
| 💾 **内存占用 (Browser 命令)** | 9 MB | 95 MB | **10.6x** |
| 📏 **二进制大小** | 4.7 MB | ~50 MB (node_modules) | **10x** |
| 🔗 **运行时依赖** | 无 | Node.js 20+ | **零依赖** |
| ✅ **测试通过率** | 103/122 (84%) | 104/122 (85%) | 接近对等 |

**⚡ 实测命令耗时对比：**

| 命令 | 🦀 opencli-rs | 📦 opencli | 加速比 |
|------|:----------:|:-------:|:------:|
| `bilibili hot` | **1.66s** | 20.1s | 🔥 **12x** |
| `zhihu hot` | **1.77s** | 20.5s | 🔥 **11.6x** |
| `xueqiu search 茅台` | **1.82s** | 9.2s | ⚡ **5x** |
| `xiaohongshu search` | **5.1s** | 14s | ⚡ **2.7x** |

> 基于 122 个命令的自动化测试（55 个站点），macOS Apple Silicon 环境。

## 特性

- **55 个站点、333 个命令** —— 覆盖 Bilibili、Twitter、Reddit、知乎、小红书、YouTube、Hacker News 等
- **浏览器会话复用** —— 通过 Chrome 扩展复用已登录状态，无需管理 token
- **声明式 YAML Pipeline** —— 用 YAML 描述数据抓取流程，零代码新增适配器
- **AI 原生发现** —— `explore` 分析网站 API、`generate` 一键生成适配器、`cascade` 探测认证策略
- **AI 智能生成** —— `generate --ai` 使用大模型分析任意网站，自动生成适配器，通过 [autocli.ai](https://autocli.ai) 云端共享
- **下载媒体和文章** —— 视频下载（yt-dlp）、文章导出为 Markdown 并本地化配图
- **外部 CLI 透传** —— 集成 GitHub CLI、Docker、Kubernetes 等工具
- **多格式输出** —— table、JSON、YAML、CSV、Markdown
- **单一二进制** —— 编译为 4MB 静态二进制，零运行时依赖

## 安装
> **只有一个文件，下载即可使用。** 无需 Node.js、Python 或任何运行时，放到 PATH 里就能跑。

### Homebrew（macOS / Linux）

```bash
brew tap nashsu/opencli-rs
brew install opencli-rs
```

### 一键安装脚本（macOS / Linux）

```bash
curl -fsSL https://raw.githubusercontent.com/nashsu/opencli-rs/main/scripts/install.sh | sh
```

自动检测系统和架构，下载对应二进制，安装到 `/usr/local/bin/`。

### Windows (PowerShell)

```powershell
Invoke-WebRequest -Uri "https://github.com/nashsu/opencli-rs/releases/latest/download/opencli-rs-x86_64-pc-windows-msvc.zip" -OutFile opencli-rs.zip
Expand-Archive opencli-rs.zip -DestinationPath .
Move-Item opencli-rs.exe "$env:LOCALAPPDATA\Microsoft\WindowsApps\"
```


### 手动下载（最简单）

从 [GitHub Releases](https://github.com/nashsu/opencli-rs/releases/latest) 下载对应平台的文件：

| 平台 | 文件 |
|------|------|
| macOS (Apple Silicon) | `opencli-rs-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `opencli-rs-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `opencli-rs-x86_64-unknown-linux-musl.tar.gz` |
| Linux (ARM64) | `opencli-rs-aarch64-unknown-linux-musl.tar.gz` |
| Windows (x64) | `opencli-rs-x86_64-pc-windows-msvc.zip` |

解压后将 `opencli-rs`（Windows 为 `opencli-rs.exe`）放到系统 PATH 中即可。

### 从源码编译

```bash
git clone https://github.com/nashsu/opencli-rs.git
cd opencli-rs
cargo build --release
cp target/release/opencli-rs /usr/local/bin/   # macOS / Linux
```

### 更新

重新运行安装命令或下载最新版本覆盖即可。

### Chrome 扩展安装（浏览器命令需要）

1. 从 [GitHub Releases](https://github.com/nashsu/opencli-rs/releases/latest) 下载 `opencli-rs-chrome-extension.zip`
2. 解压到任意目录
3. 打开 Chrome，访问 `chrome://extensions`
4. 开启右上角「开发者模式」
5. 点击「加载已解压的扩展程序」，选择解压后的文件夹
6. 扩展安装后会自动连接 opencli-rs daemon

> Public 模式命令（hackernews、devto、lobsters 等）无需安装扩展即可使用。

## Skill 安装

一键为你的 AI Agent 安装 opencli-rs skill：

```bash
npx skills add https://github.com/nashsu/opencli-rs-skill
```

## 快速开始

```bash
# 查看所有可用命令
opencli-rs --help

# 查看某个站点的命令
opencli-rs hackernews --help

# 获取 Hacker News 热门文章（公开 API，无需浏览器）
opencli-rs hackernews top --limit 10

# JSON 格式输出
opencli-rs hackernews top --limit 5 --format json

# 获取 Bilibili 热门视频（需要浏览器 + Cookie）
opencli-rs bilibili hot --limit 20

# 搜索 Twitter（需要浏览器 + 登录）
opencli-rs twitter search "rust lang" --limit 10

# 运行诊断
opencli-rs doctor

# 生成 Shell 补全
opencli-rs completion bash >> ~/.bashrc
opencli-rs completion zsh >> ~/.zshrc
opencli-rs completion fish > ~/.config/fish/completions/opencli-rs.fish
```

## AI 命令

> **由 [autocli.ai](https://autocli.ai) 提供支持** —— 获取 API Token，与社区共享适配器，让 AI 为任意网站生成适配器。

### 第一步：认证

```bash
opencli-rs auth
```

执行后会：
1. 自动打开浏览器到 [https://autocli.ai/get-token](https://autocli.ai/get-token)
2. 提示你输入 Token
3. 与服务器验证 Token 合法性
4. 保存到 `~/.opencli-rs/config.json`

### 第二步：AI 生成适配器

```bash
# AI 分析页面并生成适配器
opencli-rs generate https://www.moltbook.com/ --goal 'list' --ai

# 搜索商品
opencli-rs generate https://www.amazon.com/ --goal 'search' --ai
```

**工作流程：**
1. 从 [autocli.ai](https://autocli.ai) 搜索是否有匹配的已有适配器
2. 如果找到，显示交互式列表供选择：
   ```
   ? 找到以下已有配置，请选择:
   > [exact]   example hot (by alice) - 获取热门帖子
     [domain]  example search (by bob) - 搜索文章
     🔄 重新生成 (使用 AI 分析)
   ```
3. 如果没有匹配或选择"重新生成"，AI 分析页面（DOM 结构 + API 请求）并生成新的 YAML 适配器
4. 生成的适配器保存到本地，并上传到 [autocli.ai](https://autocli.ai) 与社区共享

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `AUTOCLI_API_BASE` | 覆盖服务器地址 | `https://www.autocli.ai` |

## 内置命令

运行 `opencli-rs --help` 查看所有可用命令。

| 站点 | 命令 | 模式 |
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

> **模式说明：** Public = 无需浏览器，直接调 API；Browser = 需要 Chrome + 扩展；Desktop = 需要桌面应用运行



## AI 发现能力

两种方式自动生成适配器：

```bash
# 🤖 AI 驱动（推荐）：大模型分析页面并生成适配器
opencli-rs generate https://www.example.com --goal hot --ai
# 优先从 autocli.ai 搜索已有适配器，未找到则使用 AI 生成

# 🔧 规则驱动：无需 AI 的启发式分析
opencli-rs generate https://www.example.com --goal hot

# 探索网站 API（端点、框架、Store）
opencli-rs explore https://www.example.com --site mysite

# 交互式模糊测试（点击按钮触发隐藏 API）
opencli-rs explore https://www.example.com --auto --click "评论,字幕"

# 自动探测认证策略（PUBLIC → COOKIE → HEADER）
opencli-rs cascade https://api.example.com/hot
```

**发现能力：**
- `.json` 后缀探测（Reddit 风格 REST 发现）
- `__INITIAL_STATE__` 提取（Bilibili、小红书等 SSR 站点）
- Pinia/Vuex Store 发现和 Action 映射
- `--goal search` 自动发现搜索端点
- 框架检测（Vue/React/Next.js/Nuxt）

## 下载

下载支持站点的媒体和文章：

```bash
# 下载 B 站视频（需要 yt-dlp）
opencli-rs bilibili download BV1xxx --output ./videos --quality 1080p

# 下载知乎文章为 Markdown（含配图）
opencli-rs zhihu download "https://zhuanlan.zhihu.com/p/xxx" --output ./articles

# 下载微信公众号文章为 Markdown（含配图）
opencli-rs weixin download "https://mp.weixin.qq.com/s/xxx" --output ./articles

# 下载 Twitter/X 媒体（图片 + 视频）
opencli-rs twitter download nash_su --limit 10 --output ./twitter
opencli-rs twitter download --tweet-url "https://x.com/user/status/123" --output ./twitter
```

**下载特性：**
- 视频通过 yt-dlp 下载（自动从浏览器提取 cookies，无需系统授权）
- 文章导出为 Markdown + YAML 头信息（标题、作者、日期、来源）
- 配图自动下载并本地化（远程 URL 替换为本地 `images/img_001.jpg`）
- 输出结构：`output/文章标题/标题.md` + `output/文章标题/images/`

## 外部 CLI 集成

已集成的外部工具（透传执行）：

| 工具 | 说明 |
|------|------|
| `gh` | GitHub CLI |
| `docker` | Docker CLI |
| `kubectl` | Kubernetes CLI |
| `obsidian` | Obsidian 笔记管理 |
| `readwise` | Readwise 阅读管理 |
| `gws` | Google Workspace CLI |

```bash
# 透传到 GitHub CLI
opencli-rs gh repo list

# 透传到 kubectl
opencli-rs kubectl get pods
```

## 输出格式

通过 `--format` 全局参数切换输出格式：

```bash
opencli-rs hackernews top --format table    # ASCII 表格（默认）
opencli-rs hackernews top --format json     # JSON
opencli-rs hackernews top --format yaml     # YAML
opencli-rs hackernews top --format csv      # CSV
opencli-rs hackernews top --format md       # Markdown 表格
```

## 认证策略

每个命令使用不同的认证策略：

| 策略 | 说明 | 是否需要浏览器 |
|------|------|--------------|
| `public` | 公开 API，无需认证 | 否 |
| `cookie` | 需要浏览器 Cookie | 是 |
| `header` | 需要特定请求头 | 是 |
| `intercept` | 需要拦截网络请求 | 是 |
| `ui` | 需要 UI 交互 | 是 |

## 自定义适配器

在 `~/.opencli-rs/adapters/` 下创建 YAML 文件即可添加自定义适配器：

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

### Pipeline 步骤

| 步骤 | 功能 | 示例 |
|------|------|------|
| `fetch` | HTTP 请求 | `fetch: https://api.example.com/data` |
| `evaluate` | 浏览器中执行 JS | `evaluate: "document.title"` |
| `navigate` | 页面导航 | `navigate: https://example.com` |
| `click` | 点击元素 | `click: "#button"` |
| `type` | 输入文本 | `type: { selector: "#input", text: "hello" }` |
| `wait` | 等待 | `wait: 2000` |
| `select` | 选取嵌套数据 | `select: data.items` |
| `map` | 数据映射 | `map: { title: "${{ item.title }}" }` |
| `filter` | 数据过滤 | `filter: "item.score > 10"` |
| `sort` | 排序 | `sort: { by: score, order: desc }` |
| `limit` | 截断 | `limit: "${{ args.limit }}"` |
| `intercept` | 网络拦截 | `intercept: { pattern: "*/api/*" }` |
| `tap` | 状态管理桥接 | `tap: { action: "store.fetch", url: "*/api/*" }` |
| `download` | 下载 | `download: { type: media }` |

### 模板表达式

Pipeline 中使用 `${{ expression }}` 语法：

```yaml
# 变量访问
"${{ args.limit }}"
"${{ item.title }}"
"${{ index + 1 }}"

# 比较和逻辑
"${{ item.score > 10 }}"
"${{ item.title && !item.deleted }}"

# 三元表达式
"${{ item.active ? 'yes' : 'no' }}"

# 管道过滤器
"${{ item.title | truncate(30) }}"
"${{ item.tags | join(', ') }}"
"${{ item.name | lower | trim }}"

# 字符串插值
"https://api.com/${{ item.id }}.json"

# Fallback
"${{ item.subtitle || 'N/A' }}"

# 数学函数
"${{ Math.min(args.limit, 50) }}"
```

**内置过滤器（16 个）：** `default`, `join`, `upper`, `lower`, `trim`, `truncate`, `replace`, `keys`, `length`, `first`, `last`, `json`, `slugify`, `sanitize`, `ext`, `basename`

## 配置

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `OPENCLI_VERBOSE` | - | 启用详细输出 |
| `OPENCLI_DAEMON_PORT` | `19825` | Daemon 端口 |
| `OPENCLI_CDP_ENDPOINT` | - | CDP 直连端点（跳过 Daemon） |
| `OPENCLI_BROWSER_COMMAND_TIMEOUT` | `60` | 命令超时（秒） |
| `OPENCLI_BROWSER_CONNECT_TIMEOUT` | `30` | 浏览器连接超时（秒） |
| `OPENCLI_BROWSER_EXPLORE_TIMEOUT` | `120` | Explore 超时（秒） |

### 文件路径

| 路径 | 说明 |
|------|------|
| `~/.opencli-rs/adapters/` | 用户自定义适配器 |
| `~/.opencli-rs/plugins/` | 用户插件 |
| `~/.opencli-rs/external-clis.yaml` | 用户外部 CLI 注册表 |

## 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                         用户 / AI Agent                         │
│                     opencli-rs <site> <command>                  │
└─────────────────────┬───────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                      CLI 层 (clap)                               │
│  main.rs → discovery → clap 动态子命令 → execution.rs            │
│  ┌───────────┐  ┌───────────────┐  ┌──────────────────┐        │
│  │ 内置命令   │  │ 站点适配器命令 │  │ 外部 CLI 透传     │        │
│  │ explore    │  │ bilibili hot  │  │ gh, docker, k8s  │        │
│  │ doctor     │  │ twitter feed  │  │                  │        │
│  └───────────┘  └───────┬───────┘  └──────────────────┘        │
└─────────────────────────┼───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                     执行引擎 (execution.rs)                      │
│               参数校验 → 能力路由 → 超时控制                      │
│                    ┌─────────┼─────────┐                        │
│                    ▼         ▼         ▼                        │
│              YAML Pipeline  Rust Func  External CLI              │
└────────────────┬────────────────────────────────────────────────┘
                 │
                 ▼
┌──────────────────────────────────────────────────────────────────┐
│  Pipeline 引擎                        浏览器桥接                  │
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
│  表达式引擎 (pest)                    Chrome 扩展 (CDP)          │
│  ${{ expr | filter }}                 chrome.debugger API        │
└──────────────────────────────────────────────────────────────────┘
```

### Workspace 结构

```
opencli-rs/
├── crates/
│   ├── opencli-rs-core/        # 核心数据模型：Strategy, CliCommand, Registry, IPage trait, Error
│   ├── opencli-rs-pipeline/    # Pipeline 引擎：pest 表达式, 执行器, 14 种步骤
│   ├── opencli-rs-browser/     # 浏览器桥接：Daemon, DaemonPage, CdpPage, DOM helpers
│   ├── opencli-rs-output/      # 输出渲染：table, json, yaml, csv, markdown
│   ├── opencli-rs-discovery/   # 适配器发现：YAML 解析, build.rs 编译时嵌入
│   ├── opencli-rs-external/    # 外部 CLI：加载, 检测, 透传执行
│   ├── opencli-rs-ai/          # AI 能力：explore, synthesize, cascade, generate
│   └── opencli-rs-cli/         # CLI 入口：clap, 执行编排, doctor, completion
├── adapters/                   # 333 个 YAML 适配器定义
│   ├── hackernews/
│   ├── bilibili/
│   ├── twitter/
│   └── ...（55 个站点）
└── resources/
    └── external-clis.yaml      # 外部 CLI 注册表
```

### 相比 TypeScript 原版的改进

| 改进项 | 原版 (TypeScript) | opencli-rs (Rust) |
|--------|-------------------|-------------------|
| 分发方式 | Node.js + npm install (~100MB) | 单一二进制 (4.1MB) |
| 启动速度 | 读 manifest JSON → 解析 → 注册 | 编译时嵌入，零文件 I/O |
| 模板引擎 | JS eval (安全隐患) | pest PEG parser (类型安全) |
| 并发 fetch | 非浏览器模式 pool=5 | FuturesUnordered, 并发度 10 |
| 错误系统 | 单一 hint 字符串 | 结构化错误链 + 多条建议 |
| HTTP 连接 | 每次 new fetch | reqwest 连接池复用 |
| 内存安全 | GC | 所有权系统，零 GC 暂停 |

## 开发

```bash
# 构建
cargo build

# 测试（166 个测试）
cargo test --workspace

# Release 构建（启用 LTO，约 4MB）
cargo build --release

# 添加新适配器
# 1. 在 adapters/<site>/ 下创建 YAML 文件
# 2. 重新编译（build.rs 自动嵌入）
cargo build
```

## 支持的站点

<details>
<summary>展开查看全部 55 个站点</summary>

| 站点 | 命令数 | 策略 |
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
| 其他 27 个站点 | ... | ... |

</details>

## Star History

<a href="https://www.star-history.com/?repos=nashsu%2Fopencli-rs&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&legend=top-left" />
 </picture>
</a>

## 许可证

Apache-2.0
