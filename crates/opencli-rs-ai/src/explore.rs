//! API discovery: navigate a page, capture network traffic, analyze JSON responses,
//! detect frameworks/stores, and infer CLI capabilities.
//!
//! This module mirrors the logic in the original TypeScript `explore.ts`.

use std::collections::{HashMap, HashSet};

use opencli_rs_core::{AutoScrollOptions, CliError, IPage, NetworkRequest, Strategy};
use serde_json::Value;
use tracing::debug;

use crate::types::{
    DiscoveredEndpoint, ExploreManifest, ExploreOptions, ExploreResult, FieldInfo,
    InferredCapability, RecommendedArg, ResponseAnalysis, StoreHint, StoreInfo, FIELD_ROLES,
    KNOWN_SITE_ALIASES, LIMIT_PARAMS, PAGINATION_PARAMS, SEARCH_PARAMS, VOLATILE_PARAMS,
};

// ── JavaScript snippets ─────────────────────────────────────────────────────

/// Framework detection: returns a Record<string, boolean> with vue3, vue2, react, nextjs, nuxt, pinia, vuex.
const FRAMEWORK_DETECT_JS: &str = r#"
(() => {
    const r = {};
    try {
        const app = document.querySelector('#app');
        r.vue3 = !!(app && app.__vue_app__);
        r.vue2 = !!(app && app.__vue__);
        r.react = !!(window.__REACT_DEVTOOLS_GLOBAL_HOOK__) || !!document.querySelector('[data-reactroot]');
        r.nextjs = !!(window.__NEXT_DATA__);
        r.nuxt = !!(window.__NUXT__);
        if (r.vue3 && app.__vue_app__) {
            const gp = app.__vue_app__.config && app.__vue_app__.config.globalProperties;
            r.pinia = !!(gp && gp.$pinia);
            r.vuex = !!(gp && gp.$store);
        }
    } catch(e) {}
    return r;
})()
"#;

/// Store discovery: returns an array of { type, id, actions, stateKeys }.
const STORE_DISCOVER_JS: &str = r#"
(() => {
    const stores = [];
    try {
        const app = document.querySelector('#app');
        if (!app || !app.__vue_app__) return stores;
        const gp = app.__vue_app__.config && app.__vue_app__.config.globalProperties;

        // Pinia stores
        const pinia = gp && gp.$pinia;
        if (pinia && pinia._s) {
            pinia._s.forEach((store, id) => {
                const actions = [];
                const stateKeys = [];
                for (const k in store) {
                    try {
                        if (k.startsWith('$') || k.startsWith('_')) continue;
                        if (typeof store[k] === 'function') actions.push(k);
                        else stateKeys.push(k);
                    } catch(e) {}
                }
                stores.push({ type: 'pinia', id, actions: actions.slice(0, 20), stateKeys: stateKeys.slice(0, 15) });
            });
        }

        // Vuex store modules
        const vuex = gp && gp.$store;
        if (vuex && vuex._modules && vuex._modules.root && vuex._modules.root._children) {
            const children = vuex._modules.root._children;
            for (const [modName, mod] of Object.entries(children)) {
                const actions = Object.keys((mod._rawModule && mod._rawModule.actions) || {}).slice(0, 20);
                const stateKeys = Object.keys(mod.state || {}).slice(0, 15);
                stores.push({ type: 'vuex', id: modName, actions, stateKeys });
            }
        }
    } catch(e) {}
    return stores;
})()
"#;

/// Interactive fuzzing: clicks interactive elements to trigger lazy network requests.
const INTERACT_FUZZ_JS: &str = r##"
(async () => {
    const sleep = (ms) => new Promise(r => setTimeout(r, ms));
    const clickables = Array.from(document.querySelectorAll(
        'button, [role="button"], [role="tab"], .tab, .btn, a[href="javascript:void(0)"], a[href="#"]'
    )).slice(0, 15);
    let clicked = 0;
    for (const el of clickables) {
        try {
            const rect = el.getBoundingClientRect();
            if (rect.width > 0 && rect.height > 0) {
                el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window }));
                clicked++;
                await sleep(300);
            }
        } catch(e) {}
    }
    return clicked;
})()
"##;

// ── Public API ──────────────────────────────────────────────────────────────

/// Explore a URL: navigate, auto-scroll, capture network traffic, and analyze.
///
/// Returns an `ExploreManifest` suitable for downstream synthesis.
pub async fn explore(
    page: &dyn IPage,
    url: &str,
    options: ExploreOptions,
) -> Result<ExploreManifest, CliError> {
    let wait_seconds = options.wait_seconds.unwrap_or(3.0);

    // Step 1: Navigate to URL
    page.goto(url, None).await?;
    page.wait_for_timeout((wait_seconds * 1000.0) as u64)
        .await?;

    // Step 2: Auto-scroll to trigger lazy loading
    let max_scrolls = options.max_scrolls.unwrap_or(5);
    let _ = page
        .auto_scroll(Some(AutoScrollOptions {
            max_scrolls: Some(max_scrolls.min(3)),
            delay_ms: Some(1500),
            ..Default::default()
        }))
        .await;

    // Step 2.5: Interactive fuzzing (if requested)
    if options.auto_fuzz.unwrap_or(false) {
        // Targeted clicks by label
        for label in &options.click_labels {
            let safe_label =
                serde_json::to_string(label).unwrap_or_else(|_| format!("\"{}\"", label));
            let click_js = format!(
                r#"
                (() => {{
                    const el = [...document.querySelectorAll('button, [role="button"], [role="tab"], a, span')]
                        .find(e => e.textContent && e.textContent.trim().includes({label}));
                    if (el) el.click();
                }})()
                "#,
                label = safe_label,
            );
            let _ = page.evaluate(&click_js).await;
            let _ = page.wait_for_timeout(1000).await;
        }
        // Blind fuzzing on generic interactive elements
        let _ = page.evaluate(INTERACT_FUZZ_JS).await;
        let _ = page.wait_for_timeout(2000).await;
    }

    // Step 3: Read page metadata
    let metadata = read_page_metadata(page).await;

    // Step 4: Capture network traffic
    let capture_network = options.capture_network.unwrap_or(true);
    let mut network: Vec<NetworkRequest> = if capture_network {
        page.get_network_requests().await.unwrap_or_default()
    } else {
        vec![]
    };

    // Step 4.5: Try .json suffix probing (like Reddit)
    // Some sites expose clean REST data by appending .json to page URLs
    probe_json_suffix(page, url, &mut network).await;

    // Step 4.6: Try __INITIAL_STATE__ extraction
    // Many SSR sites (Bilibili, Xiaohongshu) embed full page data in a global variable
    probe_initial_state(page, url, &mut network).await;

    // Step 5: For JSON endpoints missing a body, re-fetch in the page context
    re_fetch_missing_bodies(page, &mut network).await;

    // Step 6: Detect framework (returns Record<string, boolean>)
    let framework_map = detect_framework(page).await;
    let framework = framework_display_name(&framework_map);

    // Step 6.5: Discover stores (Pinia / Vuex)
    let stores = if framework_map.get("pinia").copied().unwrap_or(false)
        || framework_map.get("vuex").copied().unwrap_or(false)
    {
        discover_stores(page).await
    } else {
        vec![]
    };

    // Step 6.7: If goal is "search" and no search endpoints found, try discovering search
    let is_search_goal = options.goal.as_deref() == Some("search");
    if is_search_goal {
        let has_search = network.iter().any(|r| {
            SEARCH_PARAMS
                .iter()
                .any(|p| r.url.contains(&format!("{}=", p)))
        });
        if !has_search {
            debug!("Goal is 'search' but no search endpoints found, trying search discovery");
            // Try common search paths
            let base = url::Url::parse(url).ok();
            let origin = base
                .as_ref()
                .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
                .unwrap_or_default();

            let search_urls = vec![
                format!("{}/search?q=test", origin),
                format!("{}/api/search?q=test", origin),
                format!("{}/api/search?keyword=test", origin),
                format!("{}?q=test", url),
            ];

            for search_url in &search_urls {
                debug!("Trying search URL: {}", search_url);
                if page.goto(search_url, None).await.is_ok() {
                    let _ = page.wait_for_timeout(3000).await;
                    // Capture new network requests
                    let mut new_network = page.get_network_requests().await.unwrap_or_default();
                    re_fetch_missing_bodies(page, &mut new_network).await;
                    // Check if any new requests have search params
                    let found_search = new_network.iter().any(|r| {
                        SEARCH_PARAMS
                            .iter()
                            .any(|p| r.url.contains(&format!("{}=", p)))
                    });
                    if found_search {
                        debug!("Found search endpoints via {}", search_url);
                        network.extend(new_network);
                        break;
                    }
                }
            }
        }
    }

    // Step 7+8: Analyze endpoints
    let (analyzed, _total_count) = analyze_endpoints(&network);
    debug!("Discovered {} API endpoints", analyzed.len());

    // Step 9: Infer capabilities
    let (capabilities, top_strategy, all_auth) = infer_capabilities_from_endpoints(
        &analyzed,
        &stores,
        options.site_name.as_deref(),
        options.goal.as_deref(),
        url,
    );

    // Legacy store field: pick the first store type name
    let store_display = if !stores.is_empty() {
        Some(stores[0].store_type.clone())
    } else {
        None
    };

    // Aggregate auth indicators from endpoints
    let auth_indicators = all_auth.into_iter().collect::<Vec<_>>();

    // Use the first capability's strategy or infer from auth
    let _ = top_strategy; // used below
    let _ = capabilities; // used below

    Ok(ExploreManifest {
        url: metadata.url.unwrap_or_else(|| url.to_string()),
        title: metadata.title,
        endpoints: analyzed,
        framework,
        store: store_display,
        auth_indicators,
    })
}

/// Full explore returning the rich `ExploreResult` with capabilities and stores.
pub async fn explore_full(
    page: &dyn IPage,
    url: &str,
    options: ExploreOptions,
) -> Result<ExploreResult, CliError> {
    let wait_seconds = options.wait_seconds.unwrap_or(3.0);
    let site_name_opt = options.site_name.clone();
    let goal = options.goal.clone();

    // Step 1: Navigate
    page.goto(url, None).await?;
    page.wait_for_timeout((wait_seconds * 1000.0) as u64)
        .await?;

    // Step 2: Auto-scroll
    let max_scrolls = options.max_scrolls.unwrap_or(5);
    let _ = page
        .auto_scroll(Some(AutoScrollOptions {
            max_scrolls: Some(max_scrolls.min(3)),
            delay_ms: Some(1500),
            ..Default::default()
        }))
        .await;

    // Step 2.5: Interactive fuzzing
    if options.auto_fuzz.unwrap_or(false) {
        for label in &options.click_labels {
            let safe_label =
                serde_json::to_string(label).unwrap_or_else(|_| format!("\"{}\"", label));
            let click_js = format!(
                r#"
                (() => {{
                    const el = [...document.querySelectorAll('button, [role="button"], [role="tab"], a, span')]
                        .find(e => e.textContent && e.textContent.trim().includes({label}));
                    if (el) el.click();
                }})()
                "#,
                label = safe_label,
            );
            let _ = page.evaluate(&click_js).await;
            let _ = page.wait_for_timeout(1000).await;
        }
        let _ = page.evaluate(INTERACT_FUZZ_JS).await;
        let _ = page.wait_for_timeout(2000).await;
    }

    // Step 3: Metadata
    let metadata = read_page_metadata(page).await;

    // Step 4: Network
    let capture_network = options.capture_network.unwrap_or(true);
    let mut network: Vec<NetworkRequest> = if capture_network {
        page.get_network_requests().await.unwrap_or_default()
    } else {
        vec![]
    };

    // Step 5: Re-fetch missing bodies
    re_fetch_missing_bodies(page, &mut network).await;

    // Step 6: Framework
    let framework_map = detect_framework(page).await;

    // Step 6.5: Stores
    let stores = if framework_map.get("pinia").copied().unwrap_or(false)
        || framework_map.get("vuex").copied().unwrap_or(false)
    {
        discover_stores(page).await
    } else {
        vec![]
    };

    // Step 7+8: Analyze + infer
    let (analyzed, total_count) = analyze_endpoints(&network);
    let api_count = analyzed.len();
    let (capabilities, top_strategy, all_auth) = infer_capabilities_from_endpoints(
        &analyzed,
        &stores,
        site_name_opt.as_deref(),
        goal.as_deref(),
        url,
    );

    let site =
        site_name_opt.unwrap_or_else(|| detect_site_name(metadata.url.as_deref().unwrap_or(url)));

    Ok(ExploreResult {
        site,
        target_url: url.to_string(),
        final_url: metadata.url.unwrap_or_else(|| url.to_string()),
        title: metadata.title.unwrap_or_default(),
        framework: framework_map,
        stores,
        top_strategy,
        endpoint_count: total_count,
        api_endpoint_count: api_count,
        capabilities,
        auth_indicators: all_auth.into_iter().collect(),
        out_dir: String::new(),
    })
}

// ── Re-fetch missing response bodies ─────────────────────────────────────────

/// For JSON GET endpoints that returned 200 but have no response body,
/// re-fetch them in the page context using a hidden iframe to get a clean fetch.
/// Probe .json suffix on the current URL.
/// Sites like Reddit expose clean REST data by appending .json to page URLs.
async fn probe_json_suffix(page: &dyn IPage, url: &str, network: &mut Vec<NetworkRequest>) {
    // Only try if URL looks like a page (not already an API endpoint)
    if url.contains("/api/") || url.contains("/x/") || url.ends_with(".json") {
        return;
    }

    let json_url = if url.contains('?') {
        url.replacen("?", ".json?", 1)
    } else {
        format!("{}.json", url.trim_end_matches('/'))
    };

    debug!("Probing .json suffix: {}", json_url);
    let url_json = serde_json::to_string(&json_url).unwrap_or_default();
    let js = format!(
        r#"(async () => {{
            try {{
                const r = await fetch({}, {{ credentials: 'include' }});
                if (!r.ok) return null;
                const ct = r.headers.get('content-type') || '';
                if (!ct.includes('json')) return null;
                return await r.json();
            }} catch {{ return null; }}
        }})()"#,
        url_json,
    );
    match page.evaluate(&js).await {
        Ok(val) if !val.is_null() => {
            if let Ok(body) = serde_json::to_string(&val) {
                debug!("Found JSON data via .json suffix ({} bytes)", body.len());
                network.push(NetworkRequest {
                    url: json_url,
                    method: "GET".to_string(),
                    status: Some(200),
                    headers: {
                        let mut h = HashMap::new();
                        h.insert("content-type".to_string(), "application/json".to_string());
                        h
                    },
                    response_body: Some(body),
                    ..Default::default()
                });
            }
        }
        _ => {}
    }
}

/// Probe __INITIAL_STATE__ and similar global variables.
/// Many SSR sites embed full page data in window globals.
async fn probe_initial_state(page: &dyn IPage, _url: &str, network: &mut Vec<NetworkRequest>) {
    let js = r#"(() => {
        const candidates = [
            window.__INITIAL_STATE__,
            window.__NEXT_DATA__?.props?.pageProps,
            window.__NUXT__?.data,
            window.__SSR_DATA__,
            window.__PRELOADED_STATE__,
        ];
        for (const data of candidates) {
            if (data && typeof data === 'object' && Object.keys(data).length > 3) {
                return data;
            }
        }
        return null;
    })()"#;

    match page.evaluate(js).await {
        Ok(val) if !val.is_null() && val.is_object() => {
            if let Ok(body) = serde_json::to_string(&val) {
                if body.len() > 100 {
                    debug!("Found __INITIAL_STATE__ data ({} bytes)", body.len());
                    network.push(NetworkRequest {
                        url: "__INITIAL_STATE__".to_string(),
                        method: "SSR".to_string(),
                        status: Some(200),
                        headers: {
                            let mut h = HashMap::new();
                            h.insert("content-type".to_string(), "application/json".to_string());
                            h
                        },
                        response_body: Some(body),
                        ..Default::default()
                    });
                }
            }
        }
        _ => {}
    }
}

async fn re_fetch_missing_bodies(page: &dyn IPage, network: &mut [NetworkRequest]) {
    let mut fetched = 0;
    for entry in network.iter_mut() {
        if fetched >= 15 {
            break;
        }
        let ct = entry
            .headers
            .get("content-type")
            .cloned()
            .unwrap_or_default()
            .to_lowercase();
        let inferred_json = ct.contains("json")
            || entry.url.contains("/api/")
            || entry.url.contains("/x/")
            || entry.url.ends_with(".json");
        if entry.method == "GET"
            && entry.status.is_none_or(|s| s == 200) // Performance API returns null status
            && inferred_json
            && entry.response_body.is_none()
        {
            let url_json = serde_json::to_string(&entry.url).unwrap_or_default();
            let js = format!(
                r#"(async () => {{
                    try {{
                        const r = await fetch({url}, {{ credentials: 'include' }});
                        if (!r.ok) return null;
                        return await r.json();
                    }} catch(e) {{
                        return null;
                    }}
                }})()"#,
                url = url_json,
            );
            if let Ok(val) = page.evaluate(&js).await {
                if !val.is_null() {
                    if let Ok(s) = serde_json::to_string(&val) {
                        entry.response_body = Some(s);
                        // Also populate status and content_type since Performance API doesn't provide them
                        entry.status = Some(200);
                        if entry.headers.is_empty() {
                            entry
                                .headers
                                .insert("content-type".to_string(), "application/json".to_string());
                        }
                    }
                }
            }
            fetched += 1;
        }
    }
}

// ── Network analysis ────────────────────────────────────────────────────────

/// Filter, deduplicate, and score network endpoints.
/// Returns (analyzed endpoints sorted by score desc, total unique count).
pub(crate) fn analyze_endpoints(requests: &[NetworkRequest]) -> (Vec<DiscoveredEndpoint>, usize) {
    let mut seen: HashMap<String, DiscoveredEndpoint> = HashMap::new();

    for req in requests {
        if req.url.is_empty() {
            continue;
        }

        // Skip non-API content types
        let ct = req
            .headers
            .get("content-type")
            .cloned()
            .unwrap_or_default()
            .to_lowercase();
        if ct.contains("image/")
            || ct.contains("font/")
            || ct.contains("css")
            || ct.contains("javascript")
            || ct.contains("wasm")
        {
            continue;
        }
        // Skip error responses
        if let Some(status) = req.status {
            if status >= 400 {
                continue;
            }
        }

        let pattern = url_to_pattern(&req.url);
        let key = format!("{}:{}", req.method, pattern);
        if seen.contains_key(&key) {
            continue;
        }

        // Infer content type from URL if header is missing
        let effective_ct = if ct.is_empty() {
            if req.url.contains("/api/") || req.url.contains("/x/") || req.url.ends_with(".json") {
                "application/json".to_string()
            } else {
                String::new()
            }
        } else {
            ct
        };

        // Parse query parameters
        let query_params = extract_query_params(&req.url);
        let has_search = query_params
            .iter()
            .any(|p| SEARCH_PARAMS.contains(&p.as_str()));
        let has_pagination = query_params
            .iter()
            .any(|p| PAGINATION_PARAMS.contains(&p.as_str()));
        let has_limit = query_params
            .iter()
            .any(|p| LIMIT_PARAMS.contains(&p.as_str()));

        // Detect auth indicators from request headers
        let auth_indicators = detect_auth_indicators(&req.headers);

        // Analyze response body
        let (response_analysis, fields, sample_response) = if let Some(ref body) = req.response_body
        {
            let (ra, flds, sample) = analyze_response_body(body);
            (ra, flds, sample)
        } else {
            (None, vec![], None)
        };

        // Score the endpoint
        let score = score_endpoint(
            &effective_ct,
            &pattern,
            req.status,
            has_search,
            has_pagination,
            has_limit,
            &response_analysis,
        );

        let confidence = (score as f64 / 20.0).clamp(0.0, 1.0);
        let auth_level = infer_strategy(&auth_indicators);

        seen.insert(
            key,
            DiscoveredEndpoint {
                url: req.url.clone(),
                method: req.method.clone(),
                content_type: Some(effective_ct),
                fields,
                confidence,
                auth_level,
                sample_response,
                pattern,
                query_params,
                score,
                has_search_param: has_search,
                has_pagination_param: has_pagination,
                has_limit_param: has_limit,
                auth_indicators,
                response_analysis,
            },
        );
    }

    let total_count = seen.len();
    let mut analyzed: Vec<_> = seen.into_values().filter(|ep| ep.score >= 5).collect();
    analyzed.sort_by(|a, b| b.score.cmp(&a.score));

    (analyzed, total_count)
}

// ── Response body analysis ──────────────────────────────────────────────────

/// Analyze a JSON response body: find item arrays, detect fields, assign roles.
/// Returns (ResponseAnalysis, Vec<FieldInfo> for legacy compat, Option<Value>).
fn analyze_response_body(body: &str) -> (Option<ResponseAnalysis>, Vec<FieldInfo>, Option<Value>) {
    let value: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (None, vec![], None),
    };

    let candidates = find_item_arrays(&value, "", 0);
    if candidates.is_empty() {
        return (None, vec![], Some(value));
    }

    // Pick the candidate with the most items
    let best = candidates
        .iter()
        .max_by_key(|(_, items)| items.len())
        .unwrap();
    let (item_path, items) = best;

    let sample = items.first().copied();
    let sample_fields = sample.map(|s| flatten_fields(s, "", 2)).unwrap_or_default();

    // Detect field roles
    let detected_fields = detect_field_roles(&sample_fields);

    let ra = ResponseAnalysis {
        item_path: if item_path.is_empty() {
            None
        } else {
            Some(item_path.clone())
        },
        item_count: items.len(),
        detected_fields: detected_fields.clone(),
        sample_fields: sample_fields.clone(),
    };

    // Build legacy FieldInfo vec from the sample
    let fields = build_field_infos(sample, &detected_fields);

    (Some(ra), fields, Some(value))
}

/// Recursively find arrays of objects, returning (path, items) candidates.
fn find_item_arrays<'a>(
    value: &'a Value,
    path: &str,
    depth: usize,
) -> Vec<(String, Vec<&'a Value>)> {
    if depth > 4 {
        return vec![];
    }
    let mut candidates = Vec::new();

    match value {
        Value::Array(arr) if arr.len() >= 2 => {
            let has_objects = arr.iter().any(|v| v.is_object());
            if has_objects {
                candidates.push((path.to_string(), arr.iter().collect()));
            }
        }
        Value::Object(map) => {
            for (key, val) in map {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                candidates.extend(find_item_arrays(val, &child_path, depth + 1));
            }
        }
        _ => {}
    }

    candidates
}

/// Flatten field names from a JSON value up to `max_depth` levels.
fn flatten_fields(obj: &Value, prefix: &str, max_depth: usize) -> Vec<String> {
    if max_depth == 0 {
        return vec![];
    }
    let map = match obj.as_object() {
        Some(m) => m,
        None => return vec![],
    };
    let mut names = Vec::new();
    for (key, val) in map {
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        names.push(full.clone());
        if val.is_object() {
            names.extend(flatten_fields(val, &full, max_depth - 1));
        }
    }
    names
}

/// Detect field roles from a list of field names based on FIELD_ROLES aliases.
/// Exact match on role name takes priority over alias match.
fn detect_field_roles(sample_fields: &[String]) -> HashMap<String, String> {
    let mut detected: HashMap<String, String> = HashMap::new();
    for &(role, aliases) in FIELD_ROLES {
        // Pass 1: exact match — field name equals the role name itself (e.g. "title" == "title")
        let exact = sample_fields.iter().find(|f| {
            let leaf = f.split('.').next_back().unwrap_or("").to_lowercase();
            leaf == role
        });
        if let Some(f) = exact {
            detected.insert(role.to_string(), f.clone());
            continue;
        }

        // Pass 2: alias match — field name matches one of the aliases
        for f in sample_fields {
            let leaf = f.split('.').next_back().unwrap_or("").to_lowercase();
            if aliases.contains(&leaf.as_str()) {
                detected.insert(role.to_string(), f.clone());
                break;
            }
        }
    }
    detected
}

/// Build legacy `FieldInfo` vec from a sample object and detected roles.
fn build_field_infos(
    sample: Option<&Value>,
    detected_fields: &HashMap<String, String>,
) -> Vec<FieldInfo> {
    let obj = match sample.and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return vec![],
    };

    // Invert detected_fields: field_name -> role
    let name_to_role: HashMap<&str, &str> = detected_fields
        .iter()
        .map(|(role, name)| (name.as_str(), role.as_str()))
        .collect();

    let mut fields = Vec::new();
    for (key, val) in obj {
        let field_type = match val {
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Bool(_) => "boolean",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Null => "string",
        };
        let role = name_to_role.get(key.as_str()).map(|r| r.to_string());
        fields.push(FieldInfo {
            name: key.clone(),
            role,
            field_type: field_type.to_string(),
        });
    }
    fields
}

// ── Scoring ─────────────────────────────────────────────────────────────────

/// Score an endpoint by how likely it is to be a useful API.
/// Returns an integer score (not float).
fn score_endpoint(
    content_type: &str,
    pattern: &str,
    status: Option<u16>,
    has_search: bool,
    has_pagination: bool,
    has_limit: bool,
    response_analysis: &Option<ResponseAnalysis>,
) -> i32 {
    let mut s: i32 = 0;
    if content_type.contains("json") {
        s += 10;
    }
    if let Some(ref ra) = response_analysis {
        s += 5;
        s += (ra.item_count as i32).min(10);
        s += ra.detected_fields.len() as i32 * 2;
    }
    if pattern.contains("/api/") || pattern.contains("/x/") {
        s += 3;
    }
    if has_search {
        s += 3;
    }
    if has_pagination {
        s += 2;
    }
    if has_limit {
        s += 2;
    }
    if status == Some(200) {
        s += 2;
    }
    // Anti-Bot: penalize JSON endpoints returning empty data
    if let Some(ref ra) = response_analysis {
        if ra.item_count == 0 && content_type.contains("json") {
            s -= 3;
        }
    }
    s
}

// ── Auth detection ──────────────────────────────────────────────────────────

/// Detect auth-related indicators from request headers.
fn detect_auth_indicators(headers: &HashMap<String, String>) -> Vec<String> {
    let mut indicators = vec![];
    let keys: Vec<String> = headers.keys().map(|k| k.to_lowercase()).collect();
    if keys.iter().any(|k| k == "authorization") {
        indicators.push("bearer".to_string());
    }
    if keys
        .iter()
        .any(|k| k.starts_with("x-csrf") || k.starts_with("x-xsrf"))
    {
        indicators.push("csrf".to_string());
    }
    if keys
        .iter()
        .any(|k| k.starts_with("x-s") || k == "x-t" || k == "x-s-common")
    {
        indicators.push("signature".to_string());
    }
    indicators
}

/// Infer the auth strategy from detected indicators.
fn infer_strategy(indicators: &[String]) -> Strategy {
    if indicators.iter().any(|i| i == "signature") {
        Strategy::Intercept
    } else if indicators.iter().any(|i| i == "bearer" || i == "csrf") {
        Strategy::Header
    } else {
        Strategy::Cookie
    }
}

/// Aggregate auth indicators from all discovered endpoints (for the manifest).
#[allow(dead_code)]
fn infer_auth_indicators(endpoints: &[DiscoveredEndpoint]) -> Vec<String> {
    let mut all: Vec<String> = vec![];
    for ep in endpoints {
        for ind in &ep.auth_indicators {
            if !all.contains(ind) {
                all.push(ind.clone());
            }
        }
    }
    all
}

// ── Capability inference ────────────────────────────────────────────────────

/// Infer CLI capabilities from the top analyzed endpoints.
fn infer_capabilities_from_endpoints(
    endpoints: &[DiscoveredEndpoint],
    stores: &[StoreInfo],
    site_name: Option<&str>,
    goal: Option<&str>,
    _url: &str,
) -> (Vec<InferredCapability>, String, HashSet<String>) {
    let mut capabilities = Vec::new();
    let mut used_names: HashSet<String> = HashSet::new();

    for ep in endpoints.iter().take(8) {
        let mut cap_name = infer_capability_name(&ep.url, goal);
        if used_names.contains(&cap_name) {
            let suffix = ep
                .pattern
                .split('/')
                .rfind(|s| !s.is_empty() && !s.starts_with('{') && !s.contains('.'))
                .map(|s| s.to_string());
            cap_name = if let Some(s) = suffix {
                format!("{}_{}", cap_name, s)
            } else {
                format!("{}_{}", cap_name, used_names.len())
            };
        }
        used_names.insert(cap_name.clone());

        // Determine recommended columns from detected fields
        let mut cols: Vec<String> = Vec::new();
        if let Some(ref ra) = ep.response_analysis {
            for role in &["title", "url", "author", "score", "time"] {
                if ra.detected_fields.contains_key(*role) {
                    cols.push(role.to_string());
                }
            }
        }

        // Recommended args
        let mut args: Vec<RecommendedArg> = Vec::new();
        if ep.has_search_param {
            args.push(RecommendedArg {
                name: "keyword".to_string(),
                arg_type: "str".to_string(),
                required: true,
                default: None,
                description: Some("Search keyword".to_string()),
            });
        }
        args.push(RecommendedArg {
            name: "limit".to_string(),
            arg_type: "int".to_string(),
            required: false,
            default: Some(Value::Number(20.into())),
            description: Some("Number of items to return".to_string()),
        });
        if ep.has_pagination_param {
            args.push(RecommendedArg {
                name: "page".to_string(),
                arg_type: "int".to_string(),
                required: false,
                default: Some(Value::Number(1.into())),
                description: Some("Page number".to_string()),
            });
        }

        let ep_strategy_str = infer_strategy_str(&ep.auth_indicators);

        // Store hint: if intercept/signature and stores are available
        let store_hint = if (ep_strategy_str == "intercept"
            || ep.auth_indicators.contains(&"signature".to_string()))
            && !stores.is_empty()
        {
            find_store_hint(&cap_name, stores)
        } else {
            None
        };

        let strategy_str = if store_hint.is_some() {
            "store-action".to_string()
        } else {
            ep_strategy_str
        };

        let site_display = site_name.unwrap_or("site");

        capabilities.push(InferredCapability {
            name: cap_name.clone(),
            description: format!("{} {}", site_display, cap_name),
            strategy: strategy_str,
            confidence: (ep.score as f64 / 20.0).clamp(0.0, 1.0),
            endpoint: ep.pattern.clone(),
            item_path: ep
                .response_analysis
                .as_ref()
                .and_then(|ra| ra.item_path.clone()),
            recommended_columns: if cols.is_empty() {
                vec!["title".to_string(), "url".to_string()]
            } else {
                cols
            },
            recommended_args: args,
            store_hint,
        });
    }

    // Aggregate auth indicators
    let all_auth: HashSet<String> = endpoints
        .iter()
        .flat_map(|ep| ep.auth_indicators.iter().cloned())
        .collect();

    let top_strategy = if all_auth.contains("signature") {
        "intercept".to_string()
    } else if all_auth.contains("bearer") || all_auth.contains("csrf") {
        "header".to_string()
    } else if all_auth.is_empty() {
        "public".to_string()
    } else {
        "cookie".to_string()
    };

    (capabilities, top_strategy, all_auth)
}

/// Infer strategy as a string from auth indicators.
fn infer_strategy_str(indicators: &[String]) -> String {
    if indicators.iter().any(|i| i == "signature") {
        "intercept".to_string()
    } else if indicators.iter().any(|i| i == "bearer" || i == "csrf") {
        "header".to_string()
    } else {
        "cookie".to_string()
    }
}

/// Find a matching store action for a capability name.
fn find_store_hint(cap_name: &str, stores: &[StoreInfo]) -> Option<StoreHint> {
    let parts: Vec<&str> = cap_name.split('_').collect();
    for s in stores {
        let matching = s.actions.iter().find(|a| {
            let lower = a.to_lowercase();
            parts.iter().any(|part| lower.contains(part))
                || lower.contains("fetch")
                || lower.contains("get")
        });
        if let Some(action) = matching {
            return Some(StoreHint {
                store: s.id.clone(),
                action: action.clone(),
            });
        }
    }
    None
}

// ── Framework detection ─────────────────────────────────────────────────────

/// Detect frontend frameworks, returning a map of name -> bool.
async fn detect_framework(page: &dyn IPage) -> HashMap<String, bool> {
    match page.evaluate(FRAMEWORK_DETECT_JS).await {
        Ok(val) => {
            if let Some(obj) = val.as_object() {
                obj.iter()
                    .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
                    .collect()
            } else {
                HashMap::new()
            }
        }
        Err(_) => HashMap::new(),
    }
}

/// Pick a human-readable framework name from the detection map.
fn framework_display_name(map: &HashMap<String, bool>) -> Option<String> {
    // Priority order
    for name in &["nextjs", "nuxt", "vue3", "vue2", "react"] {
        if map.get(*name).copied().unwrap_or(false) {
            let display = match *name {
                "nextjs" => "Next.js",
                "nuxt" => "Nuxt",
                "vue3" => "Vue3",
                "vue2" => "Vue2",
                "react" => "React",
                other => other,
            };
            return Some(display.to_string());
        }
    }
    None
}

// ── Store discovery ─────────────────────────────────────────────────────────

/// Discover Pinia / Vuex stores via page.evaluate.
async fn discover_stores(page: &dyn IPage) -> Vec<StoreInfo> {
    match page.evaluate(STORE_DISCOVER_JS).await {
        Ok(val) => {
            if let Some(arr) = val.as_array() {
                arr.iter()
                    .filter_map(|item| {
                        let obj = item.as_object()?;
                        Some(StoreInfo {
                            store_type: obj.get("type")?.as_str()?.to_string(),
                            id: obj.get("id")?.as_str()?.to_string(),
                            actions: obj
                                .get("actions")
                                .and_then(|v| v.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                            state_keys: obj
                                .get("stateKeys")
                                .and_then(|v| v.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            } else {
                vec![]
            }
        }
        Err(_) => vec![],
    }
}

// ── Page metadata ───────────────────────────────────────────────────────────

struct PageMetadata {
    url: Option<String>,
    title: Option<String>,
}

async fn read_page_metadata(page: &dyn IPage) -> PageMetadata {
    let result = page
        .evaluate("(() => ({ url: window.location.href, title: document.title || '' }))()")
        .await;
    match result {
        Ok(val) => {
            let url = val
                .get("url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            let title = val
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            PageMetadata { url, title }
        }
        Err(_) => PageMetadata {
            url: None,
            title: None,
        },
    }
}

// ── URL helpers ─────────────────────────────────────────────────────────────

/// Normalize a URL into a pattern by replacing numeric/hex/BV path segments.
pub(crate) fn url_to_pattern(url: &str) -> String {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return url.to_string(),
    };
    let path = parsed.path();
    let mut normalized = String::new();
    for segment in path.split('/') {
        normalized.push('/');
        if segment.chars().all(|c| c.is_ascii_digit()) && !segment.is_empty() {
            normalized.push_str("{id}");
        } else if segment.len() >= 8 && segment.chars().all(|c| c.is_ascii_hexdigit()) {
            normalized.push_str("{hex}");
        } else if segment.starts_with("BV")
            && segment.len() == 12
            && segment[2..].chars().all(|c| c.is_ascii_alphanumeric())
        {
            normalized.push_str("{bvid}");
        } else {
            normalized.push_str(segment);
        }
    }

    // Collect non-volatile query params
    let mut params: Vec<String> = vec![];
    for (k, _) in parsed.query_pairs() {
        if !VOLATILE_PARAMS.contains(&k.as_ref()) {
            params.push(k.to_string());
        }
    }
    params.sort();

    let host = parsed.host_str().unwrap_or("");
    if params.is_empty() {
        format!("{}{}", host, normalized)
    } else {
        let qs = params
            .iter()
            .map(|k| format!("{}={{}}", k))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}{}?{}", host, normalized, qs)
    }
}

/// Extract non-volatile query parameter names from a URL.
fn extract_query_params(url: &str) -> Vec<String> {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return vec![],
    };
    parsed
        .query_pairs()
        .filter(|(k, _)| !VOLATILE_PARAMS.contains(&k.as_ref()))
        .map(|(k, _)| k.to_string())
        .collect()
}

// ── Site name detection ─────────────────────────────────────────────────────

/// Detect a short site name from a URL.
pub fn detect_site_name(url: &str) -> String {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return "site".to_string(),
    };
    let host = parsed.host_str().unwrap_or("").to_lowercase();

    // Check known aliases
    for &(alias_host, alias_name) in KNOWN_SITE_ALIASES {
        if host == alias_host {
            return alias_name.to_string();
        }
    }

    let parts: Vec<&str> = host
        .split('.')
        .filter(|p| !p.is_empty() && *p != "www")
        .collect();
    if parts.len() >= 2 {
        let last = parts[parts.len() - 1];
        if ["uk", "jp", "cn", "com"].contains(&last) && parts.len() >= 3 {
            return slugify(parts[parts.len() - 3]);
        }
        return slugify(parts[parts.len() - 2]);
    }
    parts
        .first()
        .map(|p| slugify(p))
        .unwrap_or_else(|| "site".to_string())
}

/// Slugify a string for use as a site name.
pub fn slugify(value: &str) -> String {
    let s: String = value
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "site".to_string()
    } else {
        s
    }
}

// ── Capability name inference ───────────────────────────────────────────────

/// Infer a capability name from an endpoint URL.
pub(crate) fn infer_capability_name(url: &str, goal: Option<&str>) -> String {
    if let Some(g) = goal {
        return g.to_string();
    }
    let u = url.to_lowercase();
    if u.contains("hot") || u.contains("popular") || u.contains("ranking") || u.contains("trending")
    {
        return "hot".to_string();
    }
    if u.contains("search") {
        return "search".to_string();
    }
    if u.contains("feed") || u.contains("timeline") || u.contains("dynamic") {
        return "feed".to_string();
    }
    if u.contains("comment") || u.contains("reply") {
        return "comments".to_string();
    }
    if u.contains("history") {
        return "history".to_string();
    }
    if u.contains("profile") || u.contains("userinfo") || u.contains("/me") {
        return "me".to_string();
    }
    if u.contains("favorite") || u.contains("collect") || u.contains("bookmark") {
        return "favorite".to_string();
    }
    // Try last meaningful path segment
    if let Ok(parsed) = url::Url::parse(url) {
        let segs: Vec<&str> = parsed
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|s| {
                !(s.is_empty()
                    || s.chars().all(|c| c.is_ascii_digit())
                    || s.len() >= 8 && s.chars().all(|c| c.is_ascii_hexdigit()))
            })
            .collect();
        if let Some(last) = segs.last() {
            return last
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
                .to_lowercase();
        }
    }
    "data".to_string()
}

// ── Rendering ───────────────────────────────────────────────────────────────

/// Render a human-readable summary of an ExploreResult.
pub fn render_explore_summary(result: &ExploreResult) -> String {
    let mut lines = vec![
        "opencli probe: OK".to_string(),
        format!("Site: {}", result.site),
        format!("URL: {}", result.target_url),
        format!(
            "Title: {}",
            if result.title.is_empty() {
                "(none)"
            } else {
                &result.title
            }
        ),
        format!("Strategy: {}", result.top_strategy),
        format!(
            "Endpoints: {} total, {} API",
            result.endpoint_count, result.api_endpoint_count
        ),
        format!("Capabilities: {}", result.capabilities.len()),
    ];

    for cap in result.capabilities.iter().take(5) {
        let store_info = cap
            .store_hint
            .as_ref()
            .map(|h| format!(" -> {}.{}()", h.store, h.action))
            .unwrap_or_default();
        lines.push(format!(
            "  * {} ({}, {:.0}%){}",
            cap.name,
            cap.strategy,
            cap.confidence * 100.0,
            store_info,
        ));
    }

    let fw_names: Vec<&str> = result
        .framework
        .iter()
        .filter(|(_, v)| **v)
        .map(|(k, _)| k.as_str())
        .collect();
    if !fw_names.is_empty() {
        lines.push(format!("Framework: {}", fw_names.join(", ")));
    }

    if !result.stores.is_empty() {
        lines.push(format!("Stores: {}", result.stores.len()));
        for s in result.stores.iter().take(5) {
            let actions_str = if s.actions.len() > 5 {
                format!("{}...", s.actions[..5].join(", "))
            } else {
                s.actions.join(", ")
            };
            lines.push(format!("  * {}/{}: {}", s.store_type, s.id, actions_str));
        }
    }

    if !result.out_dir.is_empty() {
        lines.push(format!("Output: {}", result.out_dir));
    }

    lines.join("\n")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_to_pattern_replaces_ids() {
        let p = url_to_pattern("https://api.example.com/v1/posts/12345/comments");
        assert!(p.contains("{id}"));
        assert!(p.contains("comments"));
    }

    #[test]
    fn test_url_to_pattern_strips_volatile_params() {
        let p = url_to_pattern("https://api.example.com/data?q=rust&_=123456&t=999");
        assert!(p.contains("q={}"));
        assert!(!p.contains("_="));
        assert!(!p.contains("t="));
    }

    #[test]
    fn test_url_to_pattern_bvid() {
        let p = url_to_pattern("https://api.bilibili.com/video/BVabc1234def/info");
        assert!(p.contains("{bvid}"));
    }

    #[test]
    fn test_detect_site_name_known_alias() {
        assert_eq!(
            detect_site_name("https://news.ycombinator.com"),
            "hackernews"
        );
        assert_eq!(detect_site_name("https://x.com/home"), "twitter");
        assert_eq!(detect_site_name("https://www.bilibili.com/hot"), "bilibili");
    }

    #[test]
    fn test_detect_site_name_generic() {
        assert_eq!(detect_site_name("https://www.example.com/foo"), "example");
        assert_eq!(detect_site_name("not-a-url"), "site");
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("  "), "site");
    }

    #[test]
    fn test_infer_capability_name_with_goal() {
        assert_eq!(
            infer_capability_name("https://example.com/api", Some("trending")),
            "trending"
        );
    }

    #[test]
    fn test_infer_capability_name_from_url() {
        assert_eq!(
            infer_capability_name("https://example.com/api/hot", None),
            "hot"
        );
        assert_eq!(
            infer_capability_name("https://example.com/api/search", None),
            "search"
        );
        assert_eq!(
            infer_capability_name("https://example.com/api/feed", None),
            "feed"
        );
    }

    #[test]
    fn test_analyze_endpoints_filters() {
        let requests = vec![
            NetworkRequest {
                url: "https://example.com/api/data".to_string(),
                method: "GET".to_string(),
                headers: {
                    let mut h = HashMap::new();
                    h.insert("content-type".to_string(), "application/json".to_string());
                    h
                },
                body: None,
                status: Some(200),
                response_body: Some(
                    r#"{"data":{"list":[{"title":"a","url":"b"},{"title":"c","url":"d"}]}}"#
                        .to_string(),
                ),
            },
            // Should be skipped: image
            NetworkRequest {
                url: "https://example.com/logo.png".to_string(),
                method: "GET".to_string(),
                headers: {
                    let mut h = HashMap::new();
                    h.insert("content-type".to_string(), "image/png".to_string());
                    h
                },
                body: None,
                status: Some(200),
                response_body: None,
            },
            // Should be skipped: 404
            NetworkRequest {
                url: "https://example.com/api/missing".to_string(),
                method: "GET".to_string(),
                headers: {
                    let mut h = HashMap::new();
                    h.insert("content-type".to_string(), "application/json".to_string());
                    h
                },
                body: None,
                status: Some(404),
                response_body: None,
            },
        ];

        let (endpoints, _total) = analyze_endpoints(&requests);
        assert_eq!(endpoints.len(), 1);
        assert!(endpoints[0].url.contains("api/data"));
        assert!(endpoints[0].confidence > 0.0);
        assert!(endpoints[0].score >= 5);
    }

    #[test]
    fn test_response_analysis() {
        let body = r#"{"data":{"list":[{"title":"Hello","url":"https://x.com","author":"alice"},{"title":"World","url":"https://y.com","author":"bob"}]}}"#;
        let (ra, fields, sample) = analyze_response_body(body);
        assert!(sample.is_some());
        assert!(!fields.is_empty());
        assert!(ra.is_some());
        let ra = ra.unwrap();
        assert_eq!(ra.item_path, Some("data.list".to_string()));
        assert_eq!(ra.item_count, 2);
        assert!(ra.detected_fields.contains_key("title"));
        assert!(ra.detected_fields.contains_key("url"));
        assert!(ra.detected_fields.contains_key("author"));
    }

    #[test]
    fn test_detect_auth_indicators() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer xyz".to_string());
        headers.insert("X-CSRF-Token".to_string(), "abc".to_string());
        let ind = detect_auth_indicators(&headers);
        assert!(ind.contains(&"bearer".to_string()));
        assert!(ind.contains(&"csrf".to_string()));
    }

    #[test]
    fn test_score_endpoint_json_with_items() {
        let ra = Some(ResponseAnalysis {
            item_path: Some("data.list".to_string()),
            item_count: 5,
            detected_fields: {
                let mut m = HashMap::new();
                m.insert("title".to_string(), "title".to_string());
                m.insert("url".to_string(), "url".to_string());
                m
            },
            sample_fields: vec!["title".to_string(), "url".to_string()],
        });
        let s = score_endpoint(
            "application/json",
            "example.com/api/data",
            Some(200),
            false,
            false,
            false,
            &ra,
        );
        // 10 (json) + 5 (has analysis) + 5 (item_count capped) + 4 (2 fields * 2) + 3 (/api/) + 2 (200) = 29
        assert_eq!(s, 29);
    }

    #[test]
    fn test_framework_detect_js_is_valid() {
        assert!(FRAMEWORK_DETECT_JS.contains("__NEXT_DATA__"));
        assert!(FRAMEWORK_DETECT_JS.contains("__vue_app__"));
        assert!(FRAMEWORK_DETECT_JS.contains("__REACT_DEVTOOLS_GLOBAL_HOOK__"));
    }

    #[test]
    fn test_store_discover_js_is_valid() {
        assert!(STORE_DISCOVER_JS.contains("$pinia"));
        assert!(STORE_DISCOVER_JS.contains("_modules"));
        assert!(STORE_DISCOVER_JS.contains("vuex"));
    }

    #[test]
    fn test_detect_field_roles() {
        let fields = vec![
            "title".to_string(),
            "url".to_string(),
            "author".to_string(),
            "created_at".to_string(),
            "random_field".to_string(),
        ];
        let roles = detect_field_roles(&fields);
        assert_eq!(roles.get("title"), Some(&"title".to_string()));
        assert_eq!(roles.get("url"), Some(&"url".to_string()));
        assert_eq!(roles.get("author"), Some(&"author".to_string()));
        assert_eq!(roles.get("time"), Some(&"created_at".to_string()));
        assert!(!roles.contains_key("random_field"));
    }
}
