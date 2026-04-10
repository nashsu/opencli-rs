/**
 * AutoCLI Selector Tool — Content Script
 * Right-side panel with entries-based rule management.
 * Design language: AutoCLI EON Systems (sharp corners, Satoshi + JetBrains Mono)
 */

(() => {
  const _PANEL_WIDTH = 320;

  // Toggle if already active
  if (window.__autocliSelectorActive) {
    const p = document.getElementById('__osp-root');
    const o = document.getElementById('__autocli-selector-overlay');
    if (p) {
      const showing = p.style.display !== 'none';
      p.style.display = showing ? 'none' : 'block';
      if (o) o.style.display = showing ? 'none' : 'block';
      if (showing) {
        document.body.style.marginRight = window.__ospOrigMarginRight || '';
        document.body.style.overflowX = window.__ospOrigOverflowX || '';
      } else {
        window.__ospOrigMarginRight = document.body.style.marginRight;
        window.__ospOrigOverflowX = document.body.style.overflowX;
        document.body.style.marginRight = _PANEL_WIDTH + 'px';
        document.body.style.overflowX = 'hidden';
      }
    }
    return;
  }
  window.__autocliSelectorActive = true;

  const SE = window.__autocliSelectorEngine;
  if (!SE) { console.error('[autocli-selector] Engine not loaded'); return; }
  const PANEL_WIDTH = _PANEL_WIDTH;

  // ─── State ────────────────────────────────────────────────────
  let mode = 'idle';
  let hoverEl = null;
  let activeEntryId = null;
  let entries = [];
  let entryIdCounter = 0;
  let generatedDone = false; // true after successful generate, reset on entry changes
  const COLORS = ['#ff571a','#4ecdc4','#45b7d1','#ffd93d','#a29bfe','#fd79a8','#96ceb4','#ff8a5c','#88d8b0','#c9b1ff'];

  // ─── i18n ─────────────────────────────────────────────────────
  let lang = navigator.language.startsWith('zh') ? 'zh' : 'en';
  const i = {
    get selector() { return lang==='zh' ? '选择器' : 'Selector'; },
    get newEntry() { return lang==='zh' ? '+ 新条目' : '+ New Entry'; },
    get save() { return lang==='zh' ? '保存' : 'Save'; },
    get edit() { return lang==='zh' ? '编辑' : 'Edit'; },
    get discard() { return lang==='zh' ? '撤销' : 'Discard'; },
    get pick() { return lang==='zh' ? '选取' : 'Pick'; },
    get generate() { return lang==='zh' ? '通过 AutoCLI.ai 生成' : 'Generate with AutoCLI.ai'; },
    get done() { return lang==='zh' ? '已完成' : 'Done'; },
    get cleaning() { return lang==='zh' ? '正在清洗页面...' : 'Cleaning DOM...'; },
    get analyzing() { return lang==='zh' ? 'AI 分析中...' : 'AI Analyzing...'; },
    get noEntries() { return lang==='zh' ? '暂无条目' : 'No entries yet'; },
    get statusInit() { return lang==='zh' ? '创建条目并选取页面元素来构建选择器。' : 'Create entries and pick elements to build selectors.'; },
    get statusPicking() { return lang==='zh' ? '点击页面中的元素（2个以上相似元素可自动检测模式）' : 'Click elements on the page (2+ similar items to detect pattern)'; },
    get stopped() { return lang==='zh' ? '已停止' : 'Stopped'; },
    get picking() { return lang==='zh' ? '选取中' : 'picking'; },
    get saved() { return lang==='zh' ? '已保存' : 'saved'; },
    get matched() { return lang==='zh' ? '匹配' : 'matched'; },
    get columns() { return lang==='zh' ? '数据列' : 'Columns'; },
    get usage() { return lang==='zh' ? '使用方式' : 'Usage'; },
    get parameters() { return lang==='zh' ? '参数说明' : 'Parameters'; },
    get privateLabel() { return lang==='zh' ? '仅保存到本地，不同步至 AutoCLI.ai' : 'Save locally only, do not sync to AutoCLI.ai'; },
    get copy() { return lang==='zh' ? '复制' : 'copy'; },
    get viewOn() { return lang==='zh' ? '在 autocli.ai 查看 →' : 'View on autocli.ai →'; },
    get synced() { return lang==='zh' ? '配置已同步保存到本地和云端，可直接使用' : 'Saved locally & synced to cloud. Ready to use.'; },
    get savedLocal() { return lang==='zh' ? '配置已保存到本地，可直接使用' : 'Saved locally. Ready to use.'; },
    get emptyResponse() { return lang==='zh' ? 'AI 返回了空内容' : i.emptyResponse; },
    get limitReached() { return lang==='zh' ? '已达到使用限制' : 'Limit Reached'; },
    get learnMore() { return lang==='zh' ? '前往 autocli.ai 了解更多 →' : 'Learn more at autocli.ai →'; },
    get pickingFor() { return lang==='zh' ? '正在为以下条目选取：' : 'Picking for'; },
    get escHint() { return lang==='zh' ? 'ESC 停止选取 · 点击选择器复制' : 'ESC stop picking · click selector to copy'; },
  };

  // ─── Shrink page ──────────────────────────────────────────────
  window.__ospOrigMarginRight = document.body.style.marginRight;
  window.__ospOrigOverflowX = document.body.style.overflowX;
  document.body.style.marginRight = PANEL_WIDTH + 'px';
  document.body.style.overflowX = 'hidden';

  // ─── Shadow DOM ───────────────────────────────────────────────
  const root = document.createElement('div');
  root.id = '__osp-root';
  root.style.cssText = `position:fixed;top:0;right:0;width:${PANEL_WIDTH}px;height:100vh;z-index:2147483647;`;
  document.documentElement.appendChild(root);

  const shadow = root.attachShadow({ mode: 'closed' });
  shadow.innerHTML = `
    <style>
      @import url('https://api.fontshare.com/v2/css?f[]=satoshi@400,500,700&f[]=jet-brains-mono@400,500&display=swap');
      :host { all:initial; }
      * { margin:0; padding:0; box-sizing:border-box; }

      .panel {
        width:${PANEL_WIDTH}px; height:100vh; background:#fbfbfb;
        border-left:1px solid #e2e2e2; display:flex; flex-direction:column;
        position:relative;
        font-family:'Satoshi',-apple-system,sans-serif; font-size:13px; color:#0f1112;
        -webkit-font-smoothing:antialiased;
      }

      /* Header */
      .header {
        display:flex; align-items:center; gap:8px; padding:12px 16px;
        border-bottom:1px solid #e2e2e2; background:#ffffff; flex-shrink:0;
      }
      .logo {
        font-family:'JetBrains Mono',monospace; font-size:14px; font-weight:700;
        color:#0f1112; letter-spacing:-0.04em; display:flex; align-items:baseline; gap:1px;
        text-decoration:none; cursor:pointer;
      }
      .logo:hover { opacity:0.7; }
      .logo-mark {
        display:inline-flex; align-items:center; justify-content:center;
        width:10px; height:12px; background:#0f1112; flex-shrink:0; align-self:center;
      }
      .logo-mark::after {
        content:''; display:block; width:2px; height:7px; background:#ff571a;
        animation:cursor-blink 1s step-end infinite;
      }
      @keyframes cursor-blink { 0%,100%{opacity:1} 50%{opacity:0} }
      .logo-cli { color:#ff571a; margin-left:-1px; }
      .logo-s { color:#aaabab; font-weight:500; margin-left:-1px; }
      .header-sep { color:#e2e2e2; font-size:14px; font-weight:300; }
      .header-sub { color:#5d5f5f; font-size:12px; font-weight:500; flex:1; }
      .icon-btn {
        background:none; border:1px solid #e2e2e2; width:28px; height:28px;
        display:flex; align-items:center; justify-content:center;
        cursor:pointer; color:#5d5f5f; font-size:13px; transition:border-color 0.2s;
      }
      .icon-btn:hover { border-color:#ff571a; color:#0f1112; }
      .lang-wrap { position:relative; }
      .icon-btn.lang {
        width:auto; padding:0 6px; gap:3px;
        font-size:10px; font-family:'JetBrains Mono',monospace;
      }
      .lang-code { font-size:10px; font-weight:600; }
      .lang-menu {
        display:none; position:absolute; top:100%; right:0; margin-top:4px;
        background:#fff; border:1px solid #e2e2e2; min-width:100px; z-index:10;
        box-shadow:0 2px 8px rgba(0,0,0,0.08);
      }
      .lang-menu.open { display:block; }
      .lang-opt {
        padding:6px 10px; font-size:11px; cursor:pointer;
        font-family:'Satoshi',sans-serif; color:#5d5f5f;
      }
      .lang-opt:hover { background:#f0f1f1; color:#0f1112; }
      .lang-opt.active { color:#ff571a; font-weight:600; }

      /* Body */
      .body { padding:12px 16px; flex:1; overflow-y:auto; padding-bottom:60px; }

      /* Footer — sticky generate button */
      .footer {
        position:absolute; bottom:0; left:0; right:0;
        padding:10px 16px; background:#fff; border-top:1px solid #e2e2e2;
      }

      /* Top bar */
      .top-bar { display:flex; gap:6px; margin-bottom:12px; }
      .btn {
        display:inline-flex; align-items:center; justify-content:center; gap:5px;
        padding:7px 14px; font-family:inherit; font-size:12px; font-weight:500;
        color:#0f1112; background:#ffffff; border:1px solid #e2e2e2;
        cursor:pointer; transition:border-color 0.2s, background 0.2s; white-space:nowrap;
      }
      .btn:hover { border-color:#ff571a; background:#f0f1f1; }
      .btn-accent { color:#ffffff; background:#ff571a; border-color:#ff571a; }
      .btn-accent:hover { opacity:0.88; background:#ff571a; border-color:#ff571a; }
      .btn-sm { padding:3px 8px; font-size:10px; }
      .btn-save { color:#fff; background:#00cc66; border-color:#00cc66; }
      .btn-save:hover { opacity:0.88; }
      .btn-edit { color:#0f1112; background:#ffd93d; border-color:#ffd93d; }
      .btn-edit:hover { opacity:0.88; }
      .btn-danger { color:#ff571a; border-color:#ff571a; background:transparent; }
      .btn-danger:hover { background:rgba(255,87,26,0.06); }

      /* Status */
      .status {
        padding:8px 12px; border:1px solid #e2e2e2; background:#ffffff;
        font-size:11px; color:#5d5f5f; margin-bottom:12px; line-height:1.5;
      }
      .status b { color:#0f1112; }
      .status.success { border-color:rgba(0,204,102,0.3); background:rgba(0,204,102,0.04); color:#0f1112; }

      /* Entry card */
      .entry {
        border:1px solid #e2e2e2; margin-bottom:8px; background:#ffffff;
        transition:border-color 0.2s;
      }
      .entry.active { border-color:#ff571a; }
      .entry.saved { border-color:rgba(0,204,102,0.4); }

      .entry-head { padding:10px 12px; }
      /* Row 1: dot + name */
      .entry-top {
        display:flex; align-items:center; gap:8px;
      }
      .entry-dot { width:10px; height:10px; flex-shrink:0; }
      .entry-name-display { font-weight:700; font-size:13px; flex:1; letter-spacing:-0.3px; }
      .entry-name-input {
        border:1px solid #e2e2e2; background:#fbfbfb; font-family:inherit;
        font-size:13px; font-weight:700; padding:2px 6px; flex:1; outline:none;
        min-width:0; letter-spacing:-0.3px;
      }
      .entry-name-input:focus { border-color:#ff571a; }

      /* Row 2: tags left + buttons right */
      .entry-bar {
        display:flex; align-items:center; margin-top:8px; gap:6px;
      }
      .entry-tags { display:flex; gap:4px; flex:1; }
      .tag {
        display:inline-flex; align-items:center; padding:2px 7px; flex-shrink:0;
        font-size:10px; font-weight:500; font-family:'JetBrains Mono',monospace;
        border:1px solid #f2f2f2; color:#aaabab; background:#f0f1f1;
      }
      .tag-picking { color:#ff571a; border-color:rgba(255,87,26,0.25); background:rgba(255,87,26,0.06); }
      .tag-saved { color:#00cc66; border-color:rgba(0,204,102,0.25); background:rgba(0,204,102,0.06); }
      .entry-actions { display:flex; gap:4px; flex-shrink:0; }

      .entry-body { padding:8px 12px; border-top:1px solid #f2f2f2; }
      .entry-sel {
        background:#0f1112; color:#e0e0e0;
        font:10px/1.4 'JetBrains Mono',monospace;
        padding:4px 8px; cursor:pointer;
        transition:background 0.15s;
        white-space:nowrap; overflow:hidden; text-overflow:ellipsis;
      }
      .entry-sel:hover { background:#1a1c1e; }
      .entry-sample {
        font-size:11px; color:#5d5f5f; margin-top:6px; line-height:1.5;
        font-family:'JetBrains Mono',monospace;
        white-space:pre-line;
      }
      .entry-save-bar { padding:8px 12px; border-top:1px solid #f2f2f2; }
      .btn-entry-save {
        width:100%; padding:6px; background:#00cc66; color:#fff;
        border:1px solid #00cc66; font-size:11px; font-weight:600;
        cursor:pointer; font-family:'Satoshi',sans-serif;
      }
      .btn-entry-save:hover { opacity:0.88; }

      /* Empty state */
      .empty {
        text-align:center; color:#aaabab; padding:24px 0; font-size:12px;
        border:1px dashed #e2e2e2; margin-bottom:8px;
      }

      /* Help */
      .help {
        font-size:10px; color:#aaabab; margin-top:10px; line-height:1.5;
        font-family:'JetBrains Mono',monospace;
      }

      /* Export */
      .section { margin-top:12px; }
      .sec-title { font-size:10px; font-weight:600; text-transform:uppercase; color:#888; margin-bottom:6px; letter-spacing:0.5px; }
      .export-bar { display:flex; gap:5px; margin-bottom:6px; }
      .export-area {
        background:#0f1112; color:#e0e0e0; font:10px/1.4 'JetBrains Mono',monospace;
        padding:8px; max-height:200px; overflow-y:auto;
        white-space:pre-wrap; word-break:break-all;
      }

      /* Generate */
      @keyframes gen-pulse {
        0%, 100% { opacity:1; }
        50% { opacity:0.6; }
      }
      @keyframes gen-slide {
        0% { background-position:200% 0; }
        100% { background-position:-200% 0; }
      }
      .private-opt {
        display:flex; align-items:center; gap:6px; margin-bottom:8px;
        font-size:10px; color:#5d5f5f; font-family:'JetBrains Mono',monospace;
        cursor:pointer; user-select:none;
      }
      .private-opt input {
        width:12px; height:12px; margin:0; cursor:pointer;
        accent-color:#ff571a;
      }
      .btn-generate {
        width:100%; padding:8px;
        background:#ff571a; color:#fff; border:1px solid #ff571a;
        font-size:12px; font-weight:600; cursor:pointer;
        font-family:'Satoshi',sans-serif; letter-spacing:0.3px;
        position:relative; overflow:hidden;
      }
      .btn-generate:hover { opacity:0.88; }
      .btn-generate:disabled { cursor:not-allowed; }
      .btn-generate.loading {
        background:linear-gradient(90deg, #5d5f5f 0%, #888 50%, #5d5f5f 100%);
        background-size:200% 100%;
        animation:gen-slide 1.5s ease infinite;
        border-color:#5d5f5f;
      }
      .generate-stream {
        background:#0f1112; color:#5d5f5f; font:10px/1.4 'JetBrains Mono',monospace;
        padding:6px 8px; margin-top:8px; height:56px; overflow:hidden;
        white-space:pre; border:1px solid #333; position:relative;
      }
      .generate-stream.active { border-color:#ff571a; color:#aaabab; }
      .generate-stream::after {
        content:''; position:absolute; bottom:0; left:0; right:0; height:16px;
        background:linear-gradient(transparent, #0f1112);
      }
      @keyframes sum-appear {
        0% { opacity:0; transform:translateY(8px); }
        100% { opacity:1; transform:translateY(0); }
      }
      .generate-summary {
        margin-top:8px; border:1px solid #e2e2e2; background:#fff; padding:12px 14px;
        animation:sum-appear 0.4s cubic-bezier(0.22,1,0.36,1);
      }
      .sum-title {
        font-size:15px; font-weight:700; color:#0f1112; letter-spacing:-0.3px;
        font-family:'JetBrains Mono',monospace;
      }
      .sum-title .sep { color:#e2e2e2; font-weight:300; margin:0 4px; }
      .sum-desc { font-size:11px; color:#5d5f5f; margin-top:2px; line-height:1.4; }
      .sum-meta { display:flex; gap:4px; flex-wrap:wrap; margin-top:8px; align-items:center; }
      .sum-meta-label { font-size:10px; color:#aaabab; font-family:'JetBrains Mono',monospace; margin-right:2px; }
      .sum-tag {
        display:inline-flex; align-items:center; padding:2px 7px;
        font-size:10px; font-weight:500; font-family:'JetBrains Mono',monospace;
        border:1px solid #f2f2f2; color:#5d5f5f; background:#f0f1f1;
      }
      .sum-tag.accent { color:#ff571a; border-color:rgba(255,87,26,0.2); background:rgba(255,87,26,0.04); }
      .sum-section-title {
        font-size:10px; font-weight:600; text-transform:uppercase; color:#aaabab;
        margin-top:10px; margin-bottom:4px; letter-spacing:0.5px;
      }
      .sum-columns { display:flex; gap:4px; flex-wrap:wrap; }
      .sum-col {
        display:inline-flex; padding:3px 8px;
        font-size:10px; font-weight:600; font-family:'JetBrains Mono',monospace;
        background:#0f1112; color:#e0e0e0;
      }
      .sum-cmd {
        display:flex; align-items:center;
        background:#0f1112; color:#e0e0e0; padding:8px 10px; margin-top:4px;
        font:12px/1.4 'JetBrains Mono',monospace; cursor:pointer;
      }
      .sum-cmd:hover { background:#1a1c1e; }
      .sum-cmd-text { flex:1; }
      .sum-cmd .arg { color:#ff571a; }
      .sum-cmd-copy {
        flex-shrink:0; color:#5d5f5f; font-size:10px;
        padding:2px 6px; border:1px solid #333;
        font-family:'JetBrains Mono',monospace;
      }
      .sum-cmd-copy:hover { color:#fff; border-color:#5d5f5f; }
      .sum-params { margin-top:4px; }
      .sum-param {
        display:flex; align-items:baseline; gap:6px; padding:4px 0;
        font-size:11px; border-bottom:1px solid #f2f2f2;
      }
      .sum-param:last-child { border-bottom:none; }
      .sum-param-name {
        font-family:'JetBrains Mono',monospace; font-weight:600; color:#0f1112;
        min-width:60px; flex-shrink:0;
      }
      .sum-param-meta {
        font-family:'JetBrains Mono',monospace; font-size:10px; color:#aaabab;
        flex-shrink:0;
      }
      .sum-param-desc { color:#5d5f5f; flex:1; }
      .sum-link {
        display:block; text-align:center; margin-top:8px;
        font-size:10px; color:#aaabab; text-decoration:none;
        font-family:'JetBrains Mono',monospace;
      }
      .sum-link:hover { color:#ff571a; }
      .sum-synced {
        text-align:center; margin-top:8px; padding-top:8px;
        border-top:1px solid #f2f2f2;
        font-size:10px; color:#aaabab; font-family:'JetBrains Mono',monospace;
      }
      .sum-synced .check { color:#00cc66; }
      .gen-notice {
        font-size:10px; margin-bottom:8px; padding:8px 10px;
        border:1px solid #e2e2e2; background:#fff;
        font-family:'JetBrains Mono',monospace; line-height:1.4;
      }
      .gen-notice.warn { border-color:#ff571a; background:rgba(255,87,26,0.04); color:#ff571a; }
      .gen-notice.info { border-color:#4ecdc4; background:rgba(78,205,196,0.04); color:#0f1112; }
      .gen-notice .notice-title { font-weight:600; margin-bottom:2px; display:flex; align-items:center; gap:5px; }
      .notice-bar { width:3px; height:12px; flex-shrink:0; }
      .notice-bar.warn { background:#ff571a; }
      .notice-bar.info { background:#4ecdc4; }
      .gen-notice .notice-link { color:#ff571a; text-decoration:none; }
      .gen-notice .notice-link:hover { text-decoration:underline; }
      .gen-notice code { background:#f0f1f1; padding:1px 4px; font-size:10px; }
      .generate-error {
        font-size:11px; margin-bottom:8px; padding:8px 10px;
        border:1px solid #ff571a; background:rgba(255,87,26,0.04);
        color:#ff571a; font-family:'JetBrains Mono',monospace;
        line-height:1.4; word-break:break-all;
      }
      .gen-ratelimit {
        border:1px solid #ffd93d; background:#fffdf5; padding:10px 12px; margin-bottom:8px;
      }
      .gen-rl-header { display:flex; align-items:center; gap:6px; }
      .gen-rl-bar { width:3px; height:14px; background:#ffd93d; flex-shrink:0; }
      .gen-rl-title { font-size:12px; font-weight:700; color:#0f1112; letter-spacing:-0.2px; }
      .gen-rl-msg { font-size:10px; color:#5d5f5f; margin-top:6px; line-height:1.5; font-family:'JetBrains Mono',monospace; }
      .gen-rl-link {
        display:block; margin-top:6px; padding-top:6px; border-top:1px solid #f2f2f2;
        font-size:10px; color:#aaabab; text-decoration:none; text-align:center;
        font-family:'JetBrains Mono',monospace;
      }
      .gen-rl-link:hover { color:#ff571a; }

      /* Toast */
      .toast {
        position:fixed; bottom:16px; left:50%; transform:translateX(-50%);
        background:#0f1112; color:#fff; padding:5px 16px;
        font-size:11px; font-family:'JetBrains Mono',monospace;
        display:none; z-index:2;
      }
    </style>

    <div class="panel">
      <div class="header">
        <a class="logo" id="s-logo" href="#" title="Open autocli.ai"><span class="logo-mark"></span>Auto<span class="logo-cli">CLI</span><span class="logo-s">.ai</span></a>
        <span class="header-sep">/</span>
        <span class="header-sub" id="s-header-sub">Selector</span>
        <div class="lang-wrap" id="s-lang-wrap">
          <button class="icon-btn lang" id="s-lang" title="Switch language">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
            <span class="lang-code" id="s-lang-code">EN</span>
          </button>
          <div class="lang-menu" id="s-lang-menu">
            <div class="lang-opt" data-lang="en">English</div>
            <div class="lang-opt" data-lang="zh">中文</div>
          </div>
        </div>
        <button class="icon-btn" id="s-close" title="Close">✕</button>
      </div>
      <div class="body">
        <div class="top-bar">
          <button class="btn btn-accent" id="s-add">+ New Entry</button>
        </div>
        <div class="status" id="s-status">Create entries and pick elements to build selectors.</div>
        <div id="s-entries">
          <div class="empty" id="s-empty">No entries yet</div>
        </div>
        <div id="s-sec-export" style="display:none;"></div>
        <div class="generate-stream" id="s-gen-stream" style="display:none;"></div>
        <div class="generate-summary" id="s-gen-summary" style="display:none;"></div>
      </div>
      <div class="footer" id="s-sec-generate">
        <div class="gen-notice" id="s-daemon-notice" style="display:none;"></div>
        <div class="gen-notice" id="s-update-notice" style="display:none;"></div>
        <div class="generate-error" id="s-gen-error" style="display:none;"></div>
        <div class="gen-ratelimit" id="s-gen-rl" style="display:none;"></div>
        <label class="private-opt"><input type="checkbox" id="s-private"> <span id="s-private-label">Private — save locally only</span></label>
        <button class="btn-generate" id="s-generate" disabled>Generate Adapter with AI</button>
      </div>
      <div class="toast" id="s-toast">copied</div>
    </div>
  `;

  // ─── Daemon proxy (via background script) ──────────────────────
  function daemonFetch(path, method, body) {
    return new Promise((resolve) => {
      chrome.runtime.sendMessage({ type: 'daemon-fetch', path, method, body }, (resp) => {
        if (chrome.runtime.lastError) {
          resolve({ ok: false, status: 0, body: '', error: chrome.runtime.lastError.message });
        } else {
          resolve(resp || { ok: false, status: 0, body: '', error: 'No response' });
        }
      });
    });
  }

  function daemonStream(path, body, onChunk, onDone, onError) {
    const port = chrome.runtime.connect({ name: 'daemon-stream' });
    port.postMessage({ path, body });
    port.onMessage.addListener((msg) => {
      if (msg.type === 'chunk') onChunk(msg.data);
      else if (msg.type === 'done') { onDone(); try { port.disconnect(); } catch {} }
      else if (msg.type === 'error') { onError(msg.status, msg.body); try { port.disconnect(); } catch {} }
    });
    port.onDisconnect.addListener(() => {
      if (chrome.runtime.lastError) onError(0, chrome.runtime.lastError.message);
    });
  }

  const q = id => shadow.getElementById(id);
  const statusEl = q('s-status');
  const entriesEl = q('s-entries');
  const emptyEl = q('s-empty');
  const toastEl = q('s-toast');
  const exportSection = q('s-sec-export');
  const genSection = q('s-sec-generate');
  const genBtn = q('s-generate');
  const privateCheckbox = q('s-private');
  const privateLabel = q('s-private-label');
  const genStream = q('s-gen-stream');
  const genSummary = q('s-gen-summary');
  const genError = q('s-gen-error');
  const genRateLimit = q('s-gen-rl');
  const daemonNotice = q('s-daemon-notice');
  const updateNotice = q('s-update-notice');
  const langBtn = q('s-lang');
  const headerSub = q('s-header-sub');
  const addBtn = q('s-add');
  const helpEl = shadow.querySelector('.help');

  const langCodeEl = q('s-lang-code');
  const langMenu = q('s-lang-menu');

  langCodeEl.textContent = lang.toUpperCase();

  // Toggle dropdown
  langBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    langMenu.classList.toggle('open');
  });

  // Close dropdown on outside click
  shadow.addEventListener('click', () => { langMenu.classList.remove('open'); });

  // Language options
  langMenu.querySelectorAll('.lang-opt').forEach(opt => {
    opt.addEventListener('click', (e) => {
      e.stopPropagation();
      lang = opt.dataset.lang;
      langCodeEl.textContent = lang.toUpperCase();
      langMenu.classList.remove('open');
      // Update active state
      langMenu.querySelectorAll('.lang-opt').forEach(o => o.classList.remove('active'));
      opt.classList.add('active');
      refreshUI();
    });
  });

  // Set initial active
  langMenu.querySelector(`[data-lang="${lang}"]`)?.classList.add('active');

  function refreshUI() {
    headerSub.textContent = i.selector;
    addBtn.textContent = i.newEntry;
    emptyEl.innerHTML = `<b>${i.noEntries}</b>`;
    if (helpEl) helpEl.innerHTML = `<b>ESC</b> ${lang==='zh' ? '停止选取 · 点击选择器复制' : 'stop picking · click selector to copy'}`;
    if (mode === 'idle' && !generatedDone) setStatus(i.statusInit, '');
    privateLabel.textContent = i.privateLabel;
    updateGenButton();
    render();
  }

  // lang toggle handled by dropdown above

  function setStatus(h, t) { statusEl.innerHTML = h; statusEl.className = 'status'+(t?' '+t:''); }
  function showToast(t) { toastEl.textContent=t||'copied'; toastEl.style.display='block'; setTimeout(()=>toastEl.style.display='none',1000); }
  function copyText(t) { navigator.clipboard.writeText(t); showToast(); }
  function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;'); }

  // ─── Overlay ──────────────────────────────────────────────────
  const overlay = document.createElement('div');
  overlay.id = '__autocli-selector-overlay';
  overlay.style.cssText = 'position:fixed;top:0;left:0;width:100%;height:100%;z-index:2147483645;pointer-events:none;';
  document.documentElement.appendChild(overlay);

  const highlights = new Map();
  function addHighlight(el, color, label, eid) {
    removeHighlight(el);
    const r = el.getBoundingClientRect();
    const d = document.createElement('div');
    d.style.cssText = `position:fixed;border:2px solid ${color};background:${color}18;pointer-events:none;z-index:2147483644;top:${r.top}px;left:${r.left}px;width:${r.width}px;height:${r.height}px;transition:all 0.15s;`;
    if (label) { const t=document.createElement('span'); t.style.cssText=`position:absolute;top:-15px;left:0;background:${color};color:#fff;font:600 9px/1 'Satoshi',sans-serif;padding:2px 5px;white-space:nowrap;`; t.textContent=label; d.appendChild(t); }
    overlay.appendChild(d); highlights.set(el, {div:d,eid});
  }
  function removeHighlight(el) { const h=highlights.get(el); if(h){h.div.remove();highlights.delete(el);} }
  function clearForEntry(eid) { for(const[el,h]of highlights){if(h.eid===eid){h.div.remove();highlights.delete(el);}} }
  function clearAllHighlights() { highlights.forEach(h=>h.div.remove()); highlights.clear(); }
  function updatePos() { highlights.forEach((h,el)=>{ const r=el.getBoundingClientRect(); Object.assign(h.div.style,{top:r.top+'px',left:r.left+'px',width:r.width+'px',height:r.height+'px'}); }); }

  const hoverDiv = document.createElement('div');
  hoverDiv.style.cssText = 'position:fixed;border:1px solid #ff571a;background:rgba(255,87,26,0.06);pointer-events:none;z-index:2147483644;display:none;';
  overlay.appendChild(hoverDiv);
  function showHover(el) { const r=el.getBoundingClientRect(); Object.assign(hoverDiv.style,{top:r.top+'px',left:r.left+'px',width:r.width+'px',height:r.height+'px',display:'block'}); }
  function hideHover() { hoverDiv.style.display='none'; }

  // ─── Events ───────────────────────────────────────────────────
  function isPanel(el) { return el && (root.contains(el) || el===root); }
  document.addEventListener('mousemove', e => { if(mode!=='picking')return; const el=e.target; if(!el||isPanel(el)||el.closest('#__autocli-selector-overlay'))return; if(el===hoverEl)return; hoverEl=el; showHover(el); }, true);
  document.addEventListener('mousedown', e => { if(mode!=='picking'||!activeEntryId)return; const el=e.target; if(!el||isPanel(el)||el.closest('#__autocli-selector-overlay'))return; e.preventDefault();e.stopPropagation();e.stopImmediatePropagation(); pickForEntry(el); }, true);
  document.addEventListener('mouseup', e => { if(mode==='picking'&&!isPanel(e.target)){e.preventDefault();e.stopPropagation();e.stopImmediatePropagation();} }, true);
  document.addEventListener('click', e => { if(mode==='picking'&&!isPanel(e.target)){e.preventDefault();e.stopPropagation();e.stopImmediatePropagation();} }, true);
  document.addEventListener('scroll', updatePos, true);
  window.addEventListener('resize', updatePos);
  document.addEventListener('keydown', e => { if(e.key==='Escape') stopPicking(); }, true);

  // ─── Entry CRUD ───────────────────────────────────────────────
  function getEntry(id) { return entries.find(e=>e.id===id); }

  function createEntry(name) {
    resetGenerated();
    const id = ++entryIdCounter;
    const color = COLORS[(id-1)%COLORS.length];
    entries.push({ id, name:name||`entry_${id}`, elements:[], selector:'', matchCount:0, color, saved:false, sample:'' });
    activateEntry(id);
    render(); return id;
  }

  function deleteEntry(id) {
    resetGenerated();
    clearForEntry(id);
    entries = entries.filter(e=>e.id!==id);
    if (activeEntryId===id) { activeEntryId=null; mode='idle'; hideHover(); }
    render(); updateExport();
  }

  function saveEntry(id) {
    const e = getEntry(id);
    if (!e) return;
    e.saved = true;
    if (activeEntryId===id) { activeEntryId=null; mode='idle'; hideHover(); }
    setStatus(`<b>${esc(e.name)}</b> ${i.saved}`, 'success');
    render(); updateExport();
  }

  // Snapshot storage for discard
  const snapshots = new Map(); // entryId -> { name, elements[], selector, matchCount, sample }

  function editEntry(id) {
    resetGenerated();
    const e = getEntry(id);
    if (!e) return;
    // Save snapshot before editing
    snapshots.set(id, { name:e.name, elements:[...e.elements], selector:e.selector, matchCount:e.matchCount, sample:e.sample });
    e.saved = false;
    activateEntry(id);
    render();
  }

  function discardEntry(id) {
    const e = getEntry(id);
    const snap = snapshots.get(id);
    if (!e || !snap) return;
    // Restore snapshot
    clearForEntry(id);
    e.name = snap.name;
    e.elements = snap.elements;
    e.selector = snap.selector;
    e.matchCount = snap.matchCount;
    e.sample = snap.sample;
    e.saved = true;
    snapshots.delete(id);
    if (activeEntryId===id) { activeEntryId=null; mode='idle'; hideHover(); }
    // Re-highlight restored elements
    e.elements.forEach(el => addHighlight(el, e.color, e.name, e.id));
    setStatus(`<b>${esc(e.name)}</b> ${lang==='zh'?'已撤销':'discarded'}`, '');
    render(); updateExport();
  }

  function activateEntry(id) {
    const e = getEntry(id);
    if (!e || e.saved) return;
    activeEntryId = id;
    mode = 'picking';
    setStatus(`${i.pickingFor} <b>${esc(e.name)}</b>`, '');
    render();
  }

  function pickForEntry(el) {
    const entry = getEntry(activeEntryId);
    if (!entry || entry.saved) return;
    const idx = entry.elements.indexOf(el);
    if (idx >= 0) { entry.elements.splice(idx,1); removeHighlight(el); }
    else { entry.elements.push(el); addHighlight(el, entry.color, entry.name, entry.id); }

    if (entry.elements.length === 1) {
      entry.selector = SE.cssSelector(entry.elements[0]);
      entry.matchCount = document.querySelectorAll(entry.selector).length;
    } else if (entry.elements.length >= 2) {
      const result = SE.computeListSelector(entry.elements);
      if (result) {
        entry.selector = result.full; entry.matchCount = result.matchCount;
        clearForEntry(entry.id);
        entry.elements.forEach(e => addHighlight(e, entry.color, entry.name, entry.id));
        result.itemElements.forEach(item => { if(!entry.elements.includes(item)) addHighlight(item, entry.color, '', entry.id); });
      } else {
        entry.selector = entry.elements.map(e=>SE.cssSelector(e)).join(', ');
        entry.matchCount = entry.elements.length;
      }
    } else { entry.selector=''; entry.matchCount=0; }

    // Collect sample lines from matched elements
    const sampleLines = entry.elements.slice(0, 3).map(el => {
      return el.textContent.trim().replace(/\s+/g, ' ').substring(0, 60);
    }).filter(Boolean);
    if (entry.matchCount > 3) sampleLines.push('…');
    entry.sample = sampleLines.join('\n');
    setStatus(`<b>${esc(entry.name)}</b> — ${entry.matchCount} ${i.matched}`, 'success');
    render(); updateExport();
  }

  function stopPicking() { mode='idle'; activeEntryId=null; hideHover(); setStatus(i.stopped,''); render(); }

  // ─── Render ───────────────────────────────────────────────────
  function render() {
    emptyEl.style.display = entries.length===0 ? 'block' : 'none';
    entriesEl.querySelectorAll('.entry').forEach(el=>el.remove());

    entries.forEach(entry => {
      const isActive = activeEntryId===entry.id;
      const card = document.createElement('div');
      card.className = 'entry' + (isActive?' active':'') + (entry.saved?' saved':'');

      const head = document.createElement('div');
      head.className = 'entry-head';

      // Row 1: dot + name + tags
      const topRow = document.createElement('div');
      topRow.className = 'entry-top';

      const dot = document.createElement('span');
      dot.className = 'entry-dot';
      dot.style.background = entry.color;
      topRow.appendChild(dot);

      if (entry.saved) {
        const nm = document.createElement('span');
        nm.className = 'entry-name-display';
        nm.textContent = entry.name;
        topRow.appendChild(nm);
      } else {
        const inp = document.createElement('input');
        inp.className = 'entry-name-input';
        inp.value = entry.name;
        inp.addEventListener('change', () => { entry.name = inp.value.trim()||entry.name; updateExport(); });
        inp.addEventListener('click', e => e.stopPropagation());
        topRow.appendChild(inp);
      }

      head.appendChild(topRow);

      // Row 2: tags (left) + buttons (right)
      const bar = document.createElement('div');
      bar.className = 'entry-bar';

      const tagsDiv = document.createElement('div');
      tagsDiv.className = 'entry-tags';

      if (isActive) {
        const tag = document.createElement('span');
        tag.className = 'tag tag-picking'; tag.textContent = i.picking;
        tagsDiv.appendChild(tag);
      } else if (entry.saved) {
        const tag = document.createElement('span');
        tag.className = 'tag tag-saved'; tag.textContent = i.saved;
        tagsDiv.appendChild(tag);
      }
      if (entry.matchCount > 0) {
        const tag = document.createElement('span');
        tag.className = 'tag'; tag.textContent = entry.matchCount + ' ' + i.matched;
        tagsDiv.appendChild(tag);
      }

      bar.appendChild(tagsDiv);

      const actions = document.createElement('div');
      actions.className = 'entry-actions';

      if (entry.saved) {
        const b = document.createElement('button');
        b.className = 'btn btn-sm btn-edit'; b.textContent = i.edit;
        b.addEventListener('click', e => { e.stopPropagation(); editEntry(entry.id); });
        actions.appendChild(b);
      } else {
        if (!isActive) {
          const b = document.createElement('button');
          b.className = 'btn btn-sm'; b.textContent = i.pick;
          b.addEventListener('click', e => { e.stopPropagation(); activateEntry(entry.id); });
          actions.appendChild(b);
        }
        // Save button moved to entry bottom
        if (snapshots.has(entry.id)) {
          const b = document.createElement('button');
          b.className = 'btn btn-sm btn-danger'; b.textContent = i.discard;
          b.addEventListener('click', e => { e.stopPropagation(); discardEntry(entry.id); });
          actions.appendChild(b);
        }
      }
      const del = document.createElement('button');
      del.className = 'btn btn-sm btn-danger'; del.textContent = '✕';
      del.addEventListener('click', e => { e.stopPropagation(); deleteEntry(entry.id); });
      actions.appendChild(del);

      bar.appendChild(actions);
      head.appendChild(bar);
      card.appendChild(head);

      // Body
      if (entry.selector) {
        const body = document.createElement('div');
        body.className = 'entry-body';
        const sel = document.createElement('div');
        sel.className = 'entry-sel';
        sel.textContent = entry.selector;
        sel.title = 'Click to copy full selector';
        sel.addEventListener('click', () => copyText(entry.selector));
        body.appendChild(sel);
        if (entry.sample) {
          const s = document.createElement('div');
          s.className = 'entry-sample';
          s.textContent = entry.sample;
          body.appendChild(s);
        }
        card.appendChild(body);

        // Save button at bottom (only for unsaved entries with selector)
        if (!entry.saved) {
          const saveBar = document.createElement('div');
          saveBar.className = 'entry-save-bar';
          const saveBtn = document.createElement('button');
          saveBtn.className = 'btn-entry-save';
          saveBtn.textContent = i.save;
          saveBtn.addEventListener('click', e => { e.stopPropagation(); saveEntry(entry.id); });
          saveBar.appendChild(saveBtn);
          card.appendChild(saveBar);
        }
      }
      entriesEl.appendChild(card);
    });
    updateGenButton();
  }

  // ─── Export ────────────────────────────────────────────────────
  function updateExport() {
    const withSelector = entries.filter(e=>e.selector);
    if (!withSelector.length) {
      window.__autocliSelectorExport = null;
      exportSection.style.display = 'none';
      updateGenButton();
      return;
    }
    const data = {
      url: location.href,
      title: document.title,
      entries: withSelector.map(e=>({ name:e.name, selector:e.selector, matchCount:e.matchCount, saved:e.saved, sample:e.sample||'' })),
    };
    window.__autocliSelectorExport = data;
    exportSection.style.display = 'none';
    updateGenButton();
  }

  function updateGenButton() {
    if (generatedDone) {
      genBtn.disabled = true;
      genBtn.textContent = i.done;
      genBtn.classList.remove('loading');
      return;
    }
    const hasSaved = entries.some(e => e.saved && e.selector);
    const hasUnsaved = entries.some(e => !e.saved && e.selector);
    genBtn.disabled = !hasSaved || hasUnsaved;
    genBtn.textContent = i.generate;
  }

  function resetGenerated() {
    generatedDone = false;
    updateGenButton();
  }

  // ─── Panel buttons ────────────────────────────────────────────
  q('s-add').addEventListener('click', () => {
    createEntry('');
  });

  // Blocks button removed from UI

  q('s-logo').addEventListener('click', (e) => {
    e.preventDefault();
    window.open('https://www.autocli.ai', '_blank');
  });

  // Export UI removed — data stored in window.__autocliSelectorExport for API calls

  // ─── Generate with AI ──────────────────────────────────────────
  genBtn.addEventListener('click', () => {
    const exportData = window.__autocliSelectorExport;
    if (!exportData) return;

    genBtn.disabled = true;
    genBtn.textContent = i.cleaning;
    genBtn.classList.add('loading');
    genStream.style.display = 'block';
    genStream.textContent = '';
    genStream.classList.add('active');
    genSummary.style.display = 'none';
    genError.style.display = 'none';
    genRateLimit.style.display = 'none';

    (async () => {
    try {
      // Step 1: Clean DOM
      let domTree = '';
      try {
        const DC = window.__autocliDomClean;
        if (DC) {
          domTree = await eval(DC.fullCleanPipelineJs({ scrollPages: 2 }));
        } else {
          domTree = document.documentElement.outerHTML.substring(0, 30000);
        }
      } catch(e) {
        domTree = document.documentElement.outerHTML.substring(0, 30000);
      }

      genBtn.textContent = i.analyzing;

      // Step 2: Build request
      const capturedData = {
        url: exportData.url,
        title: exportData.title || document.title,
        entries: exportData.entries,
        dom_tree: domTree,
      };

      // Step 3: Stream via background proxy using EventSource-like polling
      const fullContent = await new Promise((resolve, reject) => {
        let content = '';

        // Use background port for streaming
        const port = chrome.runtime.connect({ name: 'daemon-stream' });
        let sseBuffer = '';

        port.postMessage({ path: '/ai-generate', body: { captured_data: capturedData, stream: true, private: privateCheckbox.checked } });

        port.onMessage.addListener((msg) => {
          if (msg.type === 'chunk') {
            sseBuffer += msg.data;
            const lines = sseBuffer.split('\n');
            sseBuffer = lines.pop() || '';
            for (const line of lines) {
              const trimmed = line.trim();
              if (!trimmed || !trimmed.startsWith('data:')) continue;
              const data = trimmed.slice(5).trim();
              if (data === '[DONE]') continue;
              try {
                const parsed = JSON.parse(data);
                const delta = parsed.choices?.[0]?.delta?.content;
                if (delta) {
                  content += delta;
                  genStream.textContent = content.split('\n').slice(-4).join('\n');
                }
              } catch(e) {}
            }
          } else if (msg.type === 'done') {
            if (sseBuffer.trim()) {
              const lines = (sseBuffer + '\n').split('\n');
              for (const line of lines) {
                const trimmed = line.trim();
                if (!trimmed || !trimmed.startsWith('data:')) continue;
                const data = trimmed.slice(5).trim();
                if (data === '[DONE]') continue;
                try {
                  const parsed = JSON.parse(data);
                  const delta = parsed.choices?.[0]?.delta?.content;
                  if (delta) content += delta;
                } catch(e) {}
              }
            }
            resolve(content);
          } else if (msg.type === 'error') {
            if (msg.status === 429) {
              let errMsg = msg.body;
              try { const p = JSON.parse(msg.body); errMsg = p.error?.message || p.detail || msg.body; } catch(e) {}
              genStream.classList.remove('active');
              genStream.style.display = 'none';
              genRateLimit.style.display = 'block';
              genRateLimit.innerHTML = `
                <div class="gen-rl-header"><span class="gen-rl-bar"></span><span class="gen-rl-title">${i.limitReached}</span></div>
                <div class="gen-rl-msg">${esc(errMsg)}</div>
                <a class="gen-rl-link" href="https://www.autocli.ai" target="_blank">${i.learnMore}</a>
              `;
              resolve('');
            } else {
              reject(new Error(`${msg.status}: ${msg.body}`));
            }
          }
        });

        port.onDisconnect.addListener(() => {
          if (chrome.runtime.lastError) reject(new Error(chrome.runtime.lastError.message));
          else resolve(content);
        });
      });

      genStream.classList.remove('active');
      genStream.style.display = 'none';

      if (!fullContent) {
        if (genRateLimit.style.display !== 'block') {
          genError.textContent = i.emptyResponse;
          genError.style.display = 'block';
        }
        return;
      }

      // Step 4: Parse YAML and show summary
      const yaml = fullContent;
      const getField = (name) => {
        const match = yaml.match(new RegExp('^' + name + ':\\s*(.+)$', 'm'));
        return match ? match[1].trim().replace(/^["']|["']$/g, '') : '';
      };
      const site = getField('site') || '?';
      const cmdName = getField('name') || '?';
      const description = getField('description') || '';
      const domain = getField('domain') || '';

      const colMatch = yaml.match(/^columns:\s*\[([^\]]+)\]/m);
      const columns = colMatch ? colMatch[1].trim() : '';

      const tagMatch = yaml.match(/^tags:\s*\[([^\]]+)\]/m);
      const tags = tagMatch ? tagMatch[1].trim() : '';

      // Parse args with full details
      const argNames = [];
      const argDetails = [];
      const argSection = yaml.match(/^args:\n((?:  .+\n)*)/m);
      if (argSection) {
        const argBlocks = argSection[1].split(/^  (?=\w)/gm).filter(Boolean);
        for (const block of argBlocks) {
          const nameMatch = block.match(/^(\w[\w-]*):/);
          if (!nameMatch) continue;
          const name = nameMatch[1];
          argNames.push(name);
          const type = (block.match(/type:\s*(\w+)/) || [])[1] || 'str';
          const required = /required:\s*true/.test(block);
          const defMatch = block.match(/default:\s*(.+)/);
          const def = defMatch ? defMatch[1].trim() : '';
          const descMatch = block.match(/description:\s*["']?(.+?)["']?\s*$/m);
          const desc = descMatch ? descMatch[1] : '';
          argDetails.push({ name, type, required, def, desc });
        }
      }

      const argHints = argNames.filter(a => a !== 'limit').map(a => `<${a}>`).join(' ');
      const cmd = `autocli ${site} ${cmdName}${argHints ? ' ' + argHints : ''}`;

      // Build columns tags
      const colTags = columns ? columns.split(',').map(c => `<span class="sum-col">${esc(c.trim())}</span>`).join('') : '';

      // Build command with colored args
      const cmdHtml = `autocli ${esc(site)} ${esc(cmdName)}` +
        (argNames.filter(a => a !== 'limit').length ?
          argNames.filter(a => a !== 'limit').map(a => ` <span class="arg">&lt;${esc(a)}&gt;</span>`).join('') : '');

      genSummary.style.display = 'block';
      genSummary.innerHTML = `
        <div class="sum-title">${esc(site)}<span class="sep">/</span>${esc(cmdName)}</div>
        ${description ? `<div class="sum-desc">${esc(description)}</div>` : ''}
        ${domain ? `<div class="sum-meta"><span class="sum-tag accent">${esc(domain)}</span></div>` : ''}
        ${colTags ? `
          <div class="sum-section-title">${i.columns}</div>
          <div class="sum-columns">${colTags}</div>
        ` : ''}
        <div class="sum-section-title">${i.usage}</div>
        <div class="sum-cmd" title="Click to copy">
          <span class="sum-cmd-text">${cmdHtml}</span>
          <span class="sum-cmd-copy">${i.copy}</span>
        </div>
        ${argDetails.length ? `
          <div class="sum-section-title">${i.parameters}</div>
          <div class="sum-params">
            ${argDetails.map(a => `<div class="sum-param">
              <span class="sum-param-name">${esc(a.name)}</span>
              <span class="sum-param-meta">${esc(a.type)}${a.required ? ', required' : ''}${a.def ? ', default: ' + esc(a.def) : ''}</span>
              ${a.desc ? `<span class="sum-param-desc">${esc(a.desc)}</span>` : ''}
            </div>`).join('')}
          </div>
        ` : ''}
        <a class="sum-link" href="https://www.autocli.ai" target="_blank">${i.viewOn}</a>
        <div class="sum-synced"><span class="check">✓</span> ${privateCheckbox.checked ? i.savedLocal : i.synced}</div>
      `;

      genSummary.querySelector('.sum-cmd')?.addEventListener('click', () => copyText(cmd));

      // Mark as done
      generatedDone = true;
      updateGenButton();

    } catch(e) {
      genStream.classList.remove('active');
      genStream.style.display = 'none';
      genError.textContent = e.message;
      genError.style.display = 'block';
    } finally {
      if (!generatedDone) {
        genBtn.disabled = false;
        genBtn.textContent = i.generate;
      }
      genBtn.classList.remove('loading');
    }
    })();
  });

  q('s-close').addEventListener('click', () => {
    stopPicking(); clearAllHighlights();
    root.style.display='none'; overlay.style.display='none';
    document.body.style.marginRight = window.__ospOrigMarginRight||'';
    document.body.style.overflowX = window.__ospOrigOverflowX||'';
  });

  // ─── Startup checks ────────────────────────────────────────────
  (async () => {
    const isZh = () => lang === 'zh';

    // Check daemon connection
    try {
      const pingResp = await daemonFetch('/ping', 'GET');
      if (!pingResp.ok) throw new Error();
    } catch(e) {
      daemonNotice.className = 'gen-notice warn';
      daemonNotice.innerHTML = isZh()
        ? `<div class="notice-title"><span class="notice-bar warn"></span>未连接到 AutoCLI</div>请运行 <code>autocli</code> 启动服务后重试。`
        : `<div class="notice-title"><span class="notice-bar warn"></span>AutoCLI not connected</div>Run <code>autocli</code> to start the daemon and try again.`;
      daemonNotice.style.display = 'block';
      return;
    }

    // Check for updates
    try {
      const updateResp = await daemonFetch('/check-update', 'GET');
      if (updateResp.ok) {
        const data = JSON.parse(updateResp.body);
        if (data.update_available) {
          updateNotice.className = 'gen-notice info';
          const dl = data.download_url || 'https://github.com/nashsu/AutoCLI/releases';
          updateNotice.innerHTML = isZh()
            ? `<div class="notice-title"><span class="notice-bar info"></span>新版本可用: ${esc(data.latest_version)}</div>当前版本: ${esc(data.current_version)} · <a class="notice-link" href="${esc(dl)}" target="_blank">前往下载 →</a>`
            : `<div class="notice-title"><span class="notice-bar info"></span>Update available: ${esc(data.latest_version)}</div>Current: ${esc(data.current_version)} · <a class="notice-link" href="${esc(dl)}" target="_blank">Download →</a>`;
          updateNotice.style.display = 'block';
        }
      }
    } catch(e) { /* ignore update check failures */ }
  })();

  refreshUI();
  console.log('[autocli-selector] Loaded');
})();
