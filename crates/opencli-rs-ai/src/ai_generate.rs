//! AI-powered adapter generation.
//! Captures full page data (network requests + responses, metadata, framework)
//! and sends it to an LLM to generate a precise YAML adapter.

use opencli_rs_core::{CliError, IPage};
use serde_json::{json, Value};
use tracing::{debug, info};

use crate::explore::detect_site_name;
use crate::llm::generate_with_llm;

fn is_chinese_locale() -> bool {
    for var in &["LANG", "LC_ALL", "LANGUAGE"] {
        if let Ok(val) = std::env::var(var) {
            if val.to_lowercase().starts_with("zh") {
                return true;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLocale"])
            .output()
        {
            if String::from_utf8_lossy(&output.stdout).to_lowercase().starts_with("zh") {
                return true;
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "(Get-Culture).Name"])
            .output()
        {
            if String::from_utf8_lossy(&output.stdout).to_lowercase().starts_with("zh") {
                return true;
            }
        }
    }
    false
}

/// Fix evaluate block IIFE: extract JS code, fix bracket issues, ensure proper (async () => { ... })()
fn fix_evaluate_iife(yaml: &str) -> String {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut before: Vec<&str> = Vec::new();
    let mut js_lines: Vec<String> = Vec::new();
    let mut after: Vec<&str> = Vec::new();
    let mut eval_indent = 0;

    // Find evaluate block boundaries
    let mut state = 0; // 0=before, 1=in evaluate, 2=after
    for line in &lines {
        match state {
            0 => {
                if line.trim_start().starts_with("- evaluate:") {
                    before.push(line);
                    state = 1;
                } else {
                    before.push(line);
                }
            }
            1 => {
                if eval_indent == 0 && !line.trim().is_empty() {
                    eval_indent = line.len() - line.trim_start().len();
                }
                // Check if we've left the evaluate block (next pipeline step or top-level key)
                let trimmed = line.trim_start();
                let current_indent = line.len() - trimmed.len();
                if !line.trim().is_empty() && current_indent < eval_indent && !js_lines.is_empty() {
                    state = 2;
                    after.push(line);
                    continue;
                }
                // Also detect next pipeline step at same level as "- evaluate:"
                if trimmed.starts_with("- ") && current_indent < eval_indent && !js_lines.is_empty() {
                    state = 2;
                    after.push(line);
                    continue;
                }
                js_lines.push(line.to_string());
            }
            _ => {
                after.push(line);
            }
        }
    }

    if js_lines.is_empty() {
        return yaml.to_string();
    }

    // Join all JS lines, strip the indent, and clean up
    let js_code: String = js_lines.iter()
        .map(|l| {
            if l.len() > eval_indent { &l[eval_indent..] } else { l.trim() }
        })
        .collect::<Vec<&str>>()
        .join("\n");

    // Remove all misplaced })() from the middle, and trailing bare }
    let mut clean = js_code.trim().to_string();

    // Strip leading (async () => { or async () => { wrapper
    let has_paren = clean.starts_with("(async");
    if clean.starts_with("(async () => {") {
        clean = clean["(async () => {".len()..].to_string();
    } else if clean.starts_with("async () => {") {
        clean = clean["async () => {".len()..].to_string();
    }

    // Strip trailing })() or }
    let trimmed_end = clean.trim_end();
    if trimmed_end.ends_with("})()") {
        clean = trimmed_end[..trimmed_end.len() - 4].to_string();
    } else if trimmed_end.ends_with("}") {
        clean = trimmed_end[..trimmed_end.len() - 1].to_string();
    }

    // Remove any remaining })() that appear in the middle of the code
    // These are always errors — the IIFE close should only be at the end
    clean = clean.replace("      })()\n", "");
    clean = clean.replace("    })()\n", "");
    clean = clean.replace("})()\n", "");
    // Also handle })() at end of a line that's not the last
    let clean_lines: Vec<&str> = clean.lines().collect();
    let mut fixed_js: Vec<&str> = Vec::new();
    for line in &clean_lines {
        if line.trim() == "})()" {
            continue; // remove stray })()
        }
        fixed_js.push(line);
    }
    clean = fixed_js.join("\n");

    // Now verify bracket balance in the clean body
    let mut brace_depth: i32 = 0;
    for ch in clean.chars() {
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            _ => {}
        }
    }
    // Add missing closing braces if needed
    while brace_depth > 0 {
        clean.push_str("\n}");
        brace_depth -= 1;
    }
    // Remove extra closing braces from the end if needed
    while brace_depth < 0 {
        if let Some(pos) = clean.rfind('}') {
            clean = format!("{}{}", &clean[..pos], &clean[pos + 1..]);
            brace_depth += 1;
        } else {
            break;
        }
    }

    // Rebuild with proper IIFE wrapper
    let indent = " ".repeat(eval_indent);
    let mut rebuilt = String::new();
    for line in &before {
        rebuilt.push_str(line);
        rebuilt.push('\n');
    }
    rebuilt.push_str(&format!("{}(async () => {{\n", indent));
    for line in clean.trim().lines() {
        rebuilt.push_str(&format!("{}{}\n", indent, line));
    }
    rebuilt.push_str(&format!("{}}})()\n", indent));
    for line in &after {
        rebuilt.push_str(line);
        rebuilt.push('\n');
    }

    // Remove trailing newline to match original
    if rebuilt.ends_with('\n') && !yaml.ends_with('\n') {
        rebuilt.pop();
    }

    rebuilt
}

/// Fix common YAML pipeline formatting errors from LLM output.
fn fix_pipeline_yaml(yaml: &str) -> String {
    let mut lines: Vec<String> = yaml.lines().map(|l| l.to_string()).collect();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        // Fix: "- navigate: <url>" followed by "    settleMs: N" → nested format
        if let Some(stripped) = line.trim_start().strip_prefix("- navigate: ") {
            let url = stripped.trim().to_string();
            if !url.is_empty() && i + 1 < lines.len() {
                let next = lines[i + 1].trim_start().to_string();
                if next.starts_with("settleMs:") {
                    let indent = line.len() - line.trim_start().len();
                    let base = " ".repeat(indent);
                    lines[i] = format!("{}- navigate:", base);
                    lines.insert(i + 1, format!("{}    url: {}", base, url));
                    lines[i + 2] = format!("{}    {}", base, next);
                    i += 3;
                    continue;
                }
            }
        }
        i += 1;
    }
    let mut result = lines.join("\n");

    // Fix evaluate block: ensure proper IIFE structure
    // Extract JS code from evaluate block, fix brackets, rebuild
    result = fix_evaluate_iife(&result);

    // Remove duplicate "columns:" sections (keep first one)
    let mut seen_columns = false;
    let lines3: Vec<&str> = result.lines().collect();
    let mut final_lines: Vec<&str> = Vec::new();
    for line in &lines3 {
        if line.starts_with("columns:") {
            if seen_columns {
                continue; // skip duplicate
            }
            seen_columns = true;
        }
        final_lines.push(line);
    }
    if final_lines.len() != lines3.len() {
        result = final_lines.join("\n");
    }

    // Remove extra evaluate steps (keep only the first one)
    // Detect "  - evaluate:" lines — skip the 2nd+ and their content
    let lines4: Vec<&str> = result.lines().collect();
    let mut cleaned: Vec<&str> = Vec::new();
    let mut eval_count = 0;
    let mut skipping = false;
    let mut skip_indent = 0;
    for line in &lines4 {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- evaluate:") {
            eval_count += 1;
            if eval_count > 1 {
                skipping = true;
                skip_indent = line.len() - trimmed.len();
                continue;
            }
        }
        if skipping {
            // Skip lines that belong to the extra evaluate block (indented deeper or blank)
            if line.trim().is_empty() {
                continue;
            }
            let current_indent = line.len() - line.trim_start().len();
            if current_indent > skip_indent {
                continue; // still part of the extra evaluate
            }
            // Hit a line at same or lower indent — stop skipping
            skipping = false;
            // But also check if this is another pipeline step we should skip
            if trimmed.starts_with("- ") && !trimmed.starts_with("- evaluate:") {
                // This is a different step (like "- limit:"), keep it
            }
        }
        cleaned.push(line);
    }
    if eval_count > 1 {
        result = cleaned.join("\n");
    }

    result
}

/// Capture all API data from a page for AI analysis.
/// Installs fetch/XHR interceptors, navigates, scrolls, then collects everything.
pub async fn capture_page_data(
    page: &dyn IPage,
    url: &str,
) -> Result<Value, CliError> {
    info!(url = url, "Capturing page data for AI analysis");

    // Step 1: Navigate to page
    page.goto(url, None).await?;
    page.wait_for_timeout(5000).await?;

    // Step 2: Scroll to trigger lazy loading
    let _ = page.auto_scroll(Some(opencli_rs_core::AutoScrollOptions {
        max_scrolls: Some(3),
        delay_ms: Some(1500),
        ..Default::default()
    })).await;

    page.wait_for_timeout(2000).await?;

    // Step 3: Collect all data in one evaluate call
    let js = r#"(async () => {
        // Get all API URLs from Performance entries
        const perfEntries = performance.getEntriesByType('resource')
            .map(e => e.name)
            .filter(u => {
                const l = u.toLowerCase();
                return (l.includes('/api/') || l.includes('/v1/') || l.includes('/v2/')
                    || l.includes('/v3/') || l.includes('/x/') || l.includes('.json')
                    || l.includes('graphql') || l.includes('search') || l.includes('feed')
                    || l.includes('hot') || l.includes('trending') || l.includes('list')
                    || l.includes('recommend') || l.includes('query'))
                    && !l.endsWith('.js') && !l.endsWith('.css') && !l.endsWith('.png')
                    && !l.endsWith('.jpg') && !l.endsWith('.svg') && !l.endsWith('.woff2');
            });

        // Deduplicate by pathname
        const seen = new Set();
        const uniqueUrls = perfEntries.filter(url => {
            try {
                const key = new URL(url).pathname;
                if (seen.has(key)) return false;
                seen.add(key);
                return true;
            } catch { return false; }
        });

        // Re-fetch each API to get response body
        const apiResponses = [];
        for (const url of uniqueUrls.slice(0, 10)) {
            try {
                const resp = await fetch(url, { credentials: 'include' });
                if (!resp.ok) continue;
                const ct = resp.headers.get('content-type') || '';
                if (!ct.includes('json')) continue;
                const body = await resp.json();
                apiResponses.push({
                    url: url,
                    method: 'GET',
                    status: resp.status,
                    body: JSON.stringify(body).slice(0, 10000),
                });
            } catch {}
        }

        // Page metadata
        const meta = {
            url: location.href,
            title: document.title,
            description: document.querySelector('meta[name="description"]')?.content || '',
            keywords: document.querySelector('meta[name="keywords"]')?.content || '',
        };

        // Framework detection
        const app = document.querySelector('#app');
        const framework = {};
        try { framework.vue3 = !!app?.__vue_app__; } catch {}
        try { framework.pinia = !!(app?.__vue_app__?.config?.globalProperties?.$pinia); } catch {}
        try { framework.react = !!document.querySelector('[data-reactroot]') || !!window.__REACT_DEVTOOLS_GLOBAL_HOOK__; } catch {}
        try { framework.nextjs = !!window.__NEXT_DATA__; } catch {}
        try { framework.nuxt = !!window.__NUXT__; } catch {}

        // Global state variables
        const globals = {};
        try { if (window.__INITIAL_STATE__) globals.__INITIAL_STATE__ = JSON.stringify(window.__INITIAL_STATE__).slice(0, 10000); } catch {}
        try { if (window.__NEXT_DATA__) globals.__NEXT_DATA__ = JSON.stringify(window.__NEXT_DATA__).slice(0, 10000); } catch {}
        try { if (window.__NUXT__) globals.__NUXT__ = JSON.stringify(window.__NUXT__).slice(0, 10000); } catch {}

        // Capture rendered HTML of main content area
        // Try common content containers, fallback to body
        const contentEl = document.querySelector('main, #content, #app, .content, .main, article, [role="main"]') || document.body;
        // Remove script/style/svg/noscript tags to reduce size
        const clone = contentEl.cloneNode(true);
        clone.querySelectorAll('script, style, svg, noscript, iframe, link').forEach(el => el.remove());
        // Truncate to reasonable size for LLM context
        const html = clone.innerHTML.slice(0, 30000);

        return {
            meta,
            framework,
            globals,
            intercepted: apiResponses,
            perf_urls: uniqueUrls,
            html: html,
        };
    })()"#;

    let data = page.evaluate(js).await?;

    if data.is_null() {
        return Err(CliError::empty_result("Failed to capture page data"));
    }

    debug!(
        apis = data.get("intercepted").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        perf_urls = data.get("perf_urls").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        "Page data captured"
    );

    Ok(data)
}

/// AI-powered generate: capture page data → send to LLM → save YAML adapter.
pub async fn generate_with_ai(
    page: &dyn IPage,
    url: &str,
    goal: &str,
    token: &str,
) -> Result<(String, String, String), CliError> {
    // Step 1: Capture page data
    eprintln!("{}", if is_chinese_locale() { "📡 正在采集页面数据..." } else { "📡 Capturing page data..." });
    let captured = capture_page_data(page, url).await?;

    // Step 2: Detect site name
    let site = detect_site_name(url);

    // Step 3: Send to LLM via server API
    eprintln!("{}", if is_chinese_locale() { "🤖 正在发送至 AI 分析..." } else { "🤖 Sending to AI for analysis..." });
    let yaml = generate_with_llm(token, &captured, goal, &site).await?;

    // Step 4: Force site and name fields to match our detected values
    let mut fixed_yaml = yaml.clone();
    if let Some(line) = fixed_yaml.lines().find(|l| l.starts_with("site:")) {
        fixed_yaml = fixed_yaml.replacen(line, &format!("site: {}", site), 1);
    }
    if let Some(line) = fixed_yaml.lines().find(|l| l.starts_with("name:")) {
        fixed_yaml = fixed_yaml.replacen(line, &format!("name: {}", goal), 1);
    }

    // Step 5: Fix common YAML formatting errors from LLM
    fixed_yaml = fix_pipeline_yaml(&fixed_yaml);

    // Step 6: Inject page meta (title, description, keywords) into YAML header
    let meta_title = captured.get("meta")
        .and_then(|m| m.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let meta_description = captured.get("meta")
        .and_then(|m| m.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let meta_keywords = captured.get("meta")
        .and_then(|m| m.get("keywords"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    let mut meta_lines = String::new();
    if !meta_title.is_empty() {
        meta_lines.push_str(&format!("meta_title: \"{}\"\n", meta_title.replace('"', "\\\"")));
    }
    if !meta_description.is_empty() {
        meta_lines.push_str(&format!("meta_description: \"{}\"\n", meta_description.replace('"', "\\\"")));
    }
    if !meta_keywords.is_empty() {
        meta_lines.push_str(&format!("meta_keywords: \"{}\"\n", meta_keywords.replace('"', "\\\"")));
    }

    if !meta_lines.is_empty() {
        // Insert after the "site:" line
        if let Some(pos) = fixed_yaml.find('\n') {
            fixed_yaml = format!("{}\n{}{}", &fixed_yaml[..pos], meta_lines, &fixed_yaml[pos + 1..]);
        }
    }

    Ok((site, goal.to_string(), fixed_yaml))
}
