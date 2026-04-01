# opencli-rs

**[English](README.md) | [中文](README.zh.md) | [日本語](README.ja.md)**

超高速・メモリ安全なコマンドラインツール —— **1コマンドであらゆるWebサイトの情報を即座に取得**。Twitter/X、Reddit、YouTube、HackerNews、Bilibili、知乎、小紅書、 など [55以上のサイト](#組み込みコマンド)をカバーし、Electron デスクトップアプリの制御やローカル CLI ツール（`gh`、`docker`、`kubectl`）の統合もサポート。ブラウザセッションの再利用と AI ネイティブディスカバリー機能により駆動されます。

[OpenCLI](https://github.com/jackwener/opencli)（TypeScript）を **純 Rust で完全リライト**。機能は同等で、**最大12倍高速**、**メモリ使用量1/10**、**単一ファイル 4.7MB**、ランタイム依存ゼロ。

**OpenClaw/Agent の最良のパートナー** —— AI Agent にウェブ全体の情報にアクセスする能力を与え、1コマンドで55以上のサイトのリアルタイムデータを取得。
**AI Agentのために設計：** `AGENT.md` や `.cursorrules` に `opencli-rs list` を設定すれば、AI が利用可能な全ツールを自動的に発見できます。ローカル CLI を登録（`opencli-rs register mycli`）すれば、AI があなたの全ツールを完璧に呼び出せます。

**すべてのデスクトップアプリをCLI化！** あらゆる Electron アプリをコマンドラインツールに変換 —— Cursor、ChatGPT、Notion、Discord など。デスクトップアプリの再構成、スクリプト化、拡張が可能で、AI が自身をネイティブに制御でき、無限の可能性を秘めています。

## 🚀 パフォーマンス比較

| 指標 | 🦀 opencli-rs (Rust) | 📦 opencli (Node.js) | 改善 |
|------|:-----------------:|:-----------------:|:----:|
| 💾 **メモリ使用量 (Public コマンド)** | 15 MB | 99 MB | **6.6x** |
| 💾 **メモリ使用量 (Browser コマンド)** | 9 MB | 95 MB | **10.6x** |
| 📏 **バイナリサイズ** | 4.7 MB | ~50 MB (node_modules) | **10x** |
| 🔗 **ランタイム依存** | なし | Node.js 20+ | **ゼロ依存** |
| ✅ **テスト通過率** | 103/122 (84%) | 104/122 (85%) | ほぼ同等 |

**⚡ 実測コマンド所要時間比較：**

| コマンド | 🦀 opencli-rs | 📦 opencli | 高速化倍率 |
|------|:----------:|:-------:|:------:|
| `bilibili hot` | **1.66s** | 20.1s | 🔥 **12x** |
| `zhihu hot` | **1.77s** | 20.5s | 🔥 **11.6x** |
| `xueqiu search 茅台` | **1.82s** | 9.2s | ⚡ **5x** |
| `xiaohongshu search` | **5.1s** | 14s | ⚡ **2.7x** |

> 122コマンド（55サイト）の自動テストに基づく。macOS Apple Silicon 環境。

## 機能

- **55サイト、333コマンド** —— Bilibili、Twitter、Reddit、知乎、小紅書、YouTube、Hacker News などをカバー
- **ブラウザセッション再利用** —— Chrome 拡張機能でログイン済み状態を再利用、トークン管理不要
- **宣言型 YAML Pipeline** —— YAML でデータ取得フローを記述、コードゼロで新しいアダプターを追加
- **AI ネイティブディスカバリー** —— `explore` でサイト API を分析、`generate` で1コマンドでアダプターを自動生成、`cascade` で認証ストラテジーを探索
- **メディア＆記事ダウンロード** —— 動画ダウンロード（yt-dlp）、記事を Markdown にエクスポート＋画像のローカル保存
- **外部 CLI パススルー** —— GitHub CLI、Docker、Kubernetes などのツールを統合
- **複数出力フォーマット** —— table、JSON、YAML、CSV、Markdown
- **単一バイナリ** —— 4MB の静的バイナリにコンパイル、ランタイム依存ゼロ

## インストール
> **ファイルは1つだけ、ダウンロードすればすぐ使えます。** Node.js、Python やその他のランタイムは不要、PATH に配置するだけで実行可能。

### Homebrew（macOS / Linux）

```bash
brew tap nashsu/opencli-rs
brew install opencli-rs
```

### ワンライナーインストールスクリプト（macOS / Linux）

```bash
curl -fsSL https://raw.githubusercontent.com/nashsu/opencli-rs/main/scripts/install.sh | sh
```

システムとアーキテクチャを自動検出し、対応するバイナリをダウンロードして `/usr/local/bin/` にインストールします。

### Windows (PowerShell)

```powershell
Invoke-WebRequest -Uri "https://github.com/nashsu/opencli-rs/releases/latest/download/opencli-rs-x86_64-pc-windows-msvc.zip" -OutFile opencli-rs.zip
Expand-Archive opencli-rs.zip -DestinationPath .
Move-Item opencli-rs.exe "$env:LOCALAPPDATA\Microsoft\WindowsApps\"
```


### 手動ダウンロード（最も簡単）

[GitHub Releases](https://github.com/nashsu/opencli-rs/releases/latest) から対応プラットフォームのファイルをダウンロード：

| プラットフォーム | ファイル |
|------|------|
| macOS (Apple Silicon) | `opencli-rs-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `opencli-rs-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `opencli-rs-x86_64-unknown-linux-musl.tar.gz` |
| Linux (ARM64) | `opencli-rs-aarch64-unknown-linux-musl.tar.gz` |
| Windows (x64) | `opencli-rs-x86_64-pc-windows-msvc.zip` |

解凍後、`opencli-rs`（Windows は `opencli-rs.exe`）をシステム PATH に配置してください。

### ソースからビルド

```bash
git clone https://github.com/nashsu/opencli-rs.git
cd opencli-rs
cargo build --release
cp target/release/opencli-rs /usr/local/bin/   # macOS / Linux
```

### アップデート

インストールコマンドを再実行するか、最新バージョンをダウンロードして上書きしてください。

### Chrome 拡張機能のインストール（ブラウザコマンドに必要）

1. [GitHub Releases](https://github.com/nashsu/opencli-rs/releases/latest) から `opencli-rs-chrome-extension.zip` をダウンロード
2. 任意のディレクトリに解凍
3. Chrome を開き、`chrome://extensions` にアクセス
4. 右上の「デベロッパーモード」を有効化
5. 「パッケージ化されていない拡張機能を読み込む」をクリックし、解凍したフォルダを選択
6. 拡張機能は自動的に opencli-rs daemon に接続されます

> Public モードのコマンド（hackernews、devto、lobsters など）は拡張機能なしで使用できます。

## Skill インストール

ワンクリックで AI Agent に opencli-rs skill をインストール：

```bash
npx skills add https://github.com/nashsu/opencli-rs-skill
```

## クイックスタート

```bash
# 利用可能な全コマンドを表示
opencli-rs --help

# 特定サイトのコマンドを表示
opencli-rs hackernews --help

# Hacker News の人気記事を取得（公開 API、ブラウザ不要）
opencli-rs hackernews top --limit 10

# JSON 形式で出力
opencli-rs hackernews top --limit 5 --format json

# Bilibili の人気動画を取得（ブラウザ + Cookie が必要）
opencli-rs bilibili hot --limit 20

# Twitter を検索（ブラウザ + ログインが必要）
opencli-rs twitter search "rust lang" --limit 10

# 診断を実行
opencli-rs doctor

# シェル補完を生成
opencli-rs completion bash >> ~/.bashrc
opencli-rs completion zsh >> ~/.zshrc
opencli-rs completion fish > ~/.config/fish/completions/opencli-rs.fish
```


## 組み込みコマンド

`opencli-rs --help` を実行して利用可能な全コマンドを確認できます。

| サイト | コマンド | モード |
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

> **モード説明：** Public = パブリック（ブラウザ不要）、直接 API を呼び出し；Browser = ブラウザ（Chrome + 拡張機能が必要）；Desktop = デスクトップ（アプリの起動が必要）



## AI ディスカバリー機能

1コマンドで API を発見し、アダプターを自動生成、即座に利用可能：

```bash
# ワンショット：探索 + 合成 + アダプター保存
opencli-rs generate https://www.example.com --goal hot
# ✅ アダプター生成: example hot
#    保存先: ~/.opencli-rs/adapters/example/hot.yaml
#    実行: opencli-rs example hot

# Web サイトの API を探索（エンドポイント、フレームワーク、Store）
opencli-rs explore https://www.example.com --site mysite

# インタラクティブファジング（ボタンクリックで隠し API を発見）
opencli-rs explore https://www.example.com --auto --click "コメント,字幕"

# 認証ストラテジー自動検出（PUBLIC → COOKIE → HEADER）
opencli-rs cascade https://api.example.com/hot
```

**ディスカバリー機能：**
- `.json` サフィックスプローブ（Reddit 式 REST ディスカバリー）
- `__INITIAL_STATE__` 抽出（Bilibili、小紅書などの SSR サイト）
- Pinia/Vuex Store 発見とアクションマッピング
- `--goal search` による検索エンドポイント自動発見
- フレームワーク検出（Vue/React/Next.js/Nuxt）

## ダウンロード

対応サイトからメディアと記事をダウンロード：

```bash
# Bilibili 動画ダウンロード（yt-dlp が必要）
opencli-rs bilibili download BV1xxx --output ./videos --quality 1080p

# 知乎記事を Markdown でダウンロード（画像付き）
opencli-rs zhihu download "https://zhuanlan.zhihu.com/p/xxx" --output ./articles

# WeChat 公式アカウント記事を Markdown でダウンロード（画像付き）
opencli-rs weixin download "https://mp.weixin.qq.com/s/xxx" --output ./articles

# Twitter/X メディアダウンロード（画像 + 動画）
opencli-rs twitter download nash_su --limit 10 --output ./twitter
```

**ダウンロード機能：**
- yt-dlp による動画ダウンロード（ブラウザから Cookie を自動取得、システム認証不要）
- YAML フロントマター付き Markdown エクスポート（タイトル、著者、日付、出典）
- 画像の自動ダウンロードとローカル化（リモート URL をローカル `images/img_001.jpg` に置換）

## 外部 CLI 統合

統合済みの外部ツール（パススルー実行）：

| ツール | 説明 |
|------|------|
| `gh` | GitHub CLI |
| `docker` | Docker CLI |
| `kubectl` | Kubernetes CLI |
| `obsidian` | Obsidian ノート管理 |
| `readwise` | Readwise 読書管理 |
| `gws` | Google Workspace CLI |

```bash
# GitHub CLI にパススルー
opencli-rs gh repo list

# kubectl にパススルー
opencli-rs kubectl get pods
```

## 出力フォーマット

`--format` グローバルパラメータで出力フォーマットを切り替え：

```bash
opencli-rs hackernews top --format table    # ASCII テーブル（デフォルト）
opencli-rs hackernews top --format json     # JSON
opencli-rs hackernews top --format yaml     # YAML
opencli-rs hackernews top --format csv      # CSV
opencli-rs hackernews top --format md       # Markdown テーブル
```

## 認証ストラテジー

各コマンドは異なる認証ストラテジーを使用します：

| ストラテジー | 説明 | ブラウザが必要か |
|------|------|--------------|
| `public` | 公開 API、認証不要 | いいえ |
| `cookie` | ブラウザ Cookie が必要 | はい |
| `header` | 特定のリクエストヘッダーが必要 | はい |
| `intercept` | ネットワークリクエストの傍受が必要 | はい |
| `ui` | UI インタラクションが必要 | はい |

## カスタムアダプター

`~/.opencli-rs/adapters/` に YAML ファイルを作成するだけでカスタムアダプターを追加できます：

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

### Pipeline ステップ

| ステップ | 機能 | 例 |
|------|------|------|
| `fetch` | HTTP リクエスト | `fetch: https://api.example.com/data` |
| `evaluate` | ブラウザ内で JS を実行 | `evaluate: "document.title"` |
| `navigate` | ページナビゲーション | `navigate: https://example.com` |
| `click` | 要素をクリック | `click: "#button"` |
| `type` | テキスト入力 | `type: { selector: "#input", text: "hello" }` |
| `wait` | 待機 | `wait: 2000` |
| `select` | ネストデータを選択 | `select: data.items` |
| `map` | データマッピング | `map: { title: "${{ item.title }}" }` |
| `filter` | データフィルタリング | `filter: "item.score > 10"` |
| `sort` | ソート | `sort: { by: score, order: desc }` |
| `limit` | 切り詰め | `limit: "${{ args.limit }}"` |
| `intercept` | ネットワーク傍受 | `intercept: { pattern: "*/api/*" }` |
| `tap` | 状態管理ブリッジ | `tap: { action: "store.fetch", url: "*/api/*" }` |
| `download` | ダウンロード | `download: { type: media }` |

### テンプレート式

Pipeline では `${{ expression }}` 構文を使用します：

```yaml
# 変数アクセス
"${{ args.limit }}"
"${{ item.title }}"
"${{ index + 1 }}"

# 比較と論理演算
"${{ item.score > 10 }}"
"${{ item.title && !item.deleted }}"

# 三項演算子
"${{ item.active ? 'yes' : 'no' }}"

# パイプフィルター
"${{ item.title | truncate(30) }}"
"${{ item.tags | join(', ') }}"
"${{ item.name | lower | trim }}"

# 文字列補間
"https://api.com/${{ item.id }}.json"

# フォールバック
"${{ item.subtitle || 'N/A' }}"

# 数学関数
"${{ Math.min(args.limit, 50) }}"
```

**組み込みフィルター（16個）：** `default`, `join`, `upper`, `lower`, `trim`, `truncate`, `replace`, `keys`, `length`, `first`, `last`, `json`, `slugify`, `sanitize`, `ext`, `basename`

## 設定

### 環境変数

| 変数 | デフォルト値 | 説明 |
|------|--------|------|
| `OPENCLI_VERBOSE` | - | 詳細出力を有効化 |
| `OPENCLI_DAEMON_PORT` | `19825` | Daemon ポート |
| `OPENCLI_CDP_ENDPOINT` | - | CDP 直接接続エンドポイント（Daemon をスキップ） |
| `OPENCLI_BROWSER_COMMAND_TIMEOUT` | `60` | コマンドタイムアウト（秒） |
| `OPENCLI_BROWSER_CONNECT_TIMEOUT` | `30` | ブラウザ接続タイムアウト（秒） |
| `OPENCLI_BROWSER_EXPLORE_TIMEOUT` | `120` | Explore タイムアウト（秒） |

### ファイルパス

| パス | 説明 |
|------|------|
| `~/.opencli-rs/adapters/` | ユーザーカスタムアダプター |
| `~/.opencli-rs/plugins/` | ユーザープラグイン |
| `~/.opencli-rs/external-clis.yaml` | ユーザー外部 CLI レジストリ |

## アーキテクチャ

```
┌─────────────────────────────────────────────────────────────────┐
│                      ユーザー / AI Agent                         │
│                     opencli-rs <site> <command>                  │
└─────────────────────┬───────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                      CLI 層 (clap)                               │
│  main.rs → discovery → clap 動的サブコマンド → execution.rs      │
│  ┌───────────┐  ┌───────────────┐  ┌──────────────────┐        │
│  │ 組み込み   │  │ サイトアダプター│  │ 外部 CLI パススルー│        │
│  │ コマンド   │  │ コマンド       │  │                  │        │
│  │ explore    │  │ bilibili hot  │  │ gh, docker, k8s  │        │
│  │ doctor     │  │ twitter feed  │  │                  │        │
│  └───────────┘  └───────┬───────┘  └──────────────────┘        │
└─────────────────────────┼───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                     実行エンジン (execution.rs)                   │
│               パラメータ検証 → 機能ルーティング → タイムアウト制御  │
│                    ┌─────────┼─────────┐                        │
│                    ▼         ▼         ▼                        │
│              YAML Pipeline  Rust Func  External CLI              │
└────────────────┬────────────────────────────────────────────────┘
                 │
                 ▼
┌──────────────────────────────────────────────────────────────────┐
│  Pipeline エンジン                      ブラウザブリッジ           │
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
│  式エンジン (pest)                      Chrome 拡張機能 (CDP)     │
│  ${{ expr | filter }}                 chrome.debugger API        │
└──────────────────────────────────────────────────────────────────┘
```

### Workspace 構造

```
opencli-rs/
├── crates/
│   ├── opencli-rs-core/        # コアデータモデル：Strategy, CliCommand, Registry, IPage trait, Error
│   ├── opencli-rs-pipeline/    # Pipeline エンジン：pest 式, 実行器, 14種のステップ
│   ├── opencli-rs-browser/     # ブラウザブリッジ：Daemon, DaemonPage, CdpPage, DOM ヘルパー
│   ├── opencli-rs-output/      # 出力レンダリング：table, json, yaml, csv, markdown
│   ├── opencli-rs-discovery/   # アダプター発見：YAML パース, build.rs コンパイル時埋め込み
│   ├── opencli-rs-external/    # 外部 CLI：読み込み, 検出, パススルー実行
│   ├── opencli-rs-ai/          # AI 機能：explore, synthesize, cascade, generate
│   └── opencli-rs-cli/         # CLI エントリポイント：clap, 実行オーケストレーション, doctor, completion
├── adapters/                   # 333個の YAML アダプター定義
│   ├── hackernews/
│   ├── bilibili/
│   ├── twitter/
│   └── ...（55サイト）
└── resources/
    └── external-clis.yaml      # 外部 CLI レジストリ
```

### TypeScript 版からの改善点

| 改善項目 | 原版 (TypeScript) | opencli-rs (Rust) |
|--------|-------------------|-------------------|
| 配布方式 | Node.js + npm install (~100MB) | 単一バイナリ (4.1MB) |
| 起動速度 | manifest JSON 読み込み → パース → 登録 | コンパイル時埋め込み、ファイル I/O ゼロ |
| テンプレートエンジン | JS eval (セキュリティリスク) | pest PEG parser (型安全) |
| 並行 fetch | 非ブラウザモード pool=5 | FuturesUnordered, 並行度 10 |
| エラーシステム | 単一 hint 文字列 | 構造化エラーチェーン + 複数の提案 |
| HTTP 接続 | 毎回 new fetch | reqwest 接続プール再利用 |
| メモリ安全性 | GC | 所有権システム、GC 停止ゼロ |

## 開発

```bash
# ビルド
cargo build

# テスト（166テスト）
cargo test --workspace

# Release ビルド（LTO 有効、約 4MB）
cargo build --release

# 新しいアダプターを追加
# 1. adapters/<site>/ に YAML ファイルを作成
# 2. 再コンパイル（build.rs が自動で埋め込み）
cargo build
```

## サポートサイト

<details>
<summary>全55サイトを展開して表示</summary>

| サイト | コマンド数 | ストラテジー |
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
| その他27サイト | ... | ... |

</details>

## Star History

<a href="https://www.star-history.com/?repos=nashsu%2Fopencli-rs&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/image?repos=nashsu/opencli-rs&type=date&legend=top-left" />
 </picture>
</a>

## ライセンス

Apache-2.0
