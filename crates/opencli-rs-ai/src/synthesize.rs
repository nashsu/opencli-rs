//! Synthesize candidate CLIs from explore artifacts.
//! Generates evaluate-based YAML pipelines (matching hand-written adapter patterns).

use std::collections::HashSet;

use opencli_rs_core::{CliError, Strategy};
use tracing::debug;

use crate::explore::{detect_site_name, infer_capability_name};
use crate::types::{
    AdapterCandidate, DiscoveredEndpoint, ExploreManifest, FieldInfo, RecommendedArg, StoreHint,
    SynthesizeOptions, LIMIT_PARAMS, PAGINATION_PARAMS, SEARCH_PARAMS, VOLATILE_PARAMS,
};

/// Internal capability representation used during synthesis.
#[derive(Debug, Clone)]
struct SynthesizeCapability {
    name: String,
    description: String,
    strategy: Strategy,
    confidence: f64,
    endpoint: Option<String>,
    item_path: Option<String>,
    recommended_columns: Vec<String>,
    recommended_args: Vec<RecommendedArg>,
    store_hint: Option<StoreHint>,
}

/// Summary of a synthesized candidate (for result reporting).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SynthesizeCandidateSummary {
    pub name: String,
    pub strategy: String,
    pub confidence: f64,
}

/// Result of the synthesize operation.
#[derive(Debug, Clone)]
pub struct SynthesizeResult {
    pub site: String,
    pub candidate_count: usize,
    pub candidates: Vec<SynthesizeCandidateSummary>,
    pub adapter_candidates: Vec<AdapterCandidate>,
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Synthesize adapter candidates from an explore manifest.
///
/// For each discovered endpoint (treated as a capability), generates a YAML adapter
/// with a pipeline matching the original TypeScript patterns:
///   - **store-action strategy**: `navigate + tap` pipeline
///   - **browser strategy (cookie/header)**: `navigate + evaluate(fetch)` pipeline
///   - **public strategy**: `fetch + select + map + limit` pipeline
///
/// Returns top candidates sorted by confidence.
pub fn synthesize(
    manifest: &ExploreManifest,
    options: SynthesizeOptions,
) -> Result<Vec<AdapterCandidate>, CliError> {
    let site = options.site.as_deref().unwrap_or(manifest.url.as_str());
    let site_name = detect_site_name(site);

    // Build capabilities from endpoints
    let capabilities = build_capabilities_from_endpoints(manifest, options.goal.as_deref());

    // Select top N by confidence
    let top_n = 3;
    let top_caps: Vec<_> = capabilities.into_iter().take(top_n).collect();

    let mut candidates = Vec::new();
    let mut used_names = HashSet::new();

    for cap in &top_caps {
        let endpoint = choose_endpoint(cap, &manifest.endpoints);
        let endpoint = match endpoint {
            Some(ep) => ep,
            None => continue,
        };

        let mut cap_name = cap.name.clone();
        if used_names.contains(&cap_name) {
            let suffix = url_last_segment(&endpoint.url);
            cap_name = if let Some(s) = suffix {
                format!("{}_{}", cap_name, s)
            } else {
                format!("{}_{}", cap_name, used_names.len())
            };
        }
        used_names.insert(cap_name.clone());

        let yaml = build_candidate_yaml(&site_name, manifest, cap, endpoint);
        let description = format!(
            "{} (auto-generated)",
            if cap.description.is_empty() {
                format!("{} {}", site_name, cap_name)
            } else {
                cap.description.clone()
            }
        );

        candidates.push(AdapterCandidate {
            site: site_name.clone(),
            name: cap_name,
            description,
            strategy: cap.strategy,
            yaml,
            confidence: cap.confidence,
        });
    }

    // Sort by confidence descending
    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    debug!(
        "Synthesized {} adapter candidates for {}",
        candidates.len(),
        site_name
    );
    Ok(candidates)
}

/// Render a human-readable summary of the synthesize result.
pub fn render_synthesize_summary(result: &SynthesizeResult) -> String {
    let mut lines = vec![
        "opencli synthesize: OK".to_string(),
        format!("Site: {}", result.site),
        format!("Candidates: {}", result.candidate_count),
    ];
    for c in &result.candidates {
        lines.push(format!(
            "  - {} ({}, {:.0}% confidence)",
            c.name,
            c.strategy,
            c.confidence * 100.0,
        ));
    }
    lines.join("\n")
}

// ── Capability building ─────────────────────────────────────────────────────

/// Build SynthesizeCapability entries from discovered endpoints.
fn build_capabilities_from_endpoints(
    manifest: &ExploreManifest,
    goal: Option<&str>,
) -> Vec<SynthesizeCapability> {
    let mut endpoints = manifest.endpoints.clone();
    // When goal is "search", boost endpoints with search params
    let is_search_goal = goal == Some("search");
    endpoints.sort_by(|a, b| {
        if is_search_goal {
            // Prioritize endpoints with search params
            let a_search = if a.has_search_param { 1.0 } else { 0.0 };
            let b_search = if b.has_search_param { 1.0 } else { 0.0 };
            (b.confidence + b_search)
                .partial_cmp(&(a.confidence + a_search))
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    let mut caps: Vec<SynthesizeCapability> = Vec::new();
    let mut seen_names = HashSet::new();

    for ep in endpoints.iter().take(8) {
        let mut cap_name = infer_capability_name(&ep.url, goal);
        if seen_names.contains(&cap_name) {
            let suffix = url_last_segment(&ep.url);
            cap_name = if let Some(s) = suffix {
                format!("{}_{}", cap_name, s)
            } else {
                format!("{}_{}", cap_name, seen_names.len())
            };
        }
        seen_names.insert(cap_name.clone());

        // Build recommended args from URL query params + goal hint
        let recommended_args = build_recommended_args(&ep.url, &ep.fields, goal);

        // Build recommended columns using response_analysis if available
        let recommended_columns = if let Some(ref ra) = ep.response_analysis {
            infer_columns_from_analysis(ra, &ep.fields)
        } else {
            infer_columns(&ep.fields)
        };

        // Detect item_path from response_analysis or sample response
        let item_path: Option<String> = ep
            .response_analysis
            .as_ref()
            .and_then(|ra| ra.item_path.clone())
            .or_else(|| detect_item_path(&ep.sample_response));

        caps.push(SynthesizeCapability {
            name: cap_name.clone(),
            description: cap_name.to_string(),
            strategy: ep.auth_level,
            confidence: ep.confidence,
            endpoint: Some(ep.url.clone()),
            item_path,
            recommended_columns,
            recommended_args,
            store_hint: detect_store_hint(manifest),
        });
    }

    caps.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    caps
}

/// Choose the best matching endpoint for a capability.
fn choose_endpoint<'a>(
    cap: &SynthesizeCapability,
    endpoints: &'a [DiscoveredEndpoint],
) -> Option<&'a DiscoveredEndpoint> {
    if endpoints.is_empty() {
        return None;
    }
    // Match by endpoint URL from capability
    if let Some(ref ep_url) = cap.endpoint {
        let matched = endpoints
            .iter()
            .find(|e| e.url == *ep_url || e.url.contains(ep_url.as_str()));
        if matched.is_some() {
            return matched;
        }
    }
    // Fallback: highest scoring endpoint
    endpoints.iter().max_by(|a, b| {
        a.confidence
            .partial_cmp(&b.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

// ── URL templating ──────────────────────────────────────────────────────────

/// Build a templated URL with Jinja-style arg placeholders.
///
/// Replaces search params with `${{ args.keyword }}`, limit params with
/// `${{ args.limit | default(20) }}`, page params with `${{ args.page | default(1) }}`,
/// and strips volatile params.
fn build_templated_url(raw_url: &str, has_keyword_arg: bool) -> String {
    let parsed = match url::Url::parse(raw_url) {
        Ok(u) => u,
        Err(_) => return raw_url.to_string(),
    };
    let base = format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or(""),
        parsed.path()
    );

    let mut params = Vec::new();
    for (k, v) in parsed.query_pairs() {
        if VOLATILE_PARAMS.contains(&k.as_ref()) {
            continue;
        }
        if has_keyword_arg && SEARCH_PARAMS.contains(&k.as_ref()) {
            params.push(format!("{}=${{{{ args.keyword }}}}", k));
        } else if LIMIT_PARAMS.contains(&k.as_ref()) {
            params.push(format!("{}=${{{{ args.limit | default(20) }}}}", k));
        } else if PAGINATION_PARAMS.contains(&k.as_ref()) {
            params.push(format!("{}=${{{{ args.page | default(1) }}}}", k));
        } else {
            params.push(format!("{}={}", k, v));
        }
    }

    if params.is_empty() {
        base
    } else {
        format!("{}?{}", base, params.join("&"))
    }
}

// ── Evaluate script generation ──────────────────────────────────────────────

/// Build inline evaluate script for browser-based fetch+parse.
/// Follows patterns from bilibili/hot.yaml and twitter/trending.yaml.
fn build_evaluate_script(
    url: &str,
    item_path: &str,
    _fields: &[FieldInfo],
    _columns: &[String],
    _detected_fields: &std::collections::HashMap<String, String>,
) -> String {
    let path_chain: String = item_path.split('.').map(|p| format!("?.{}", p)).collect();

    // Don't do .map() in evaluate — let the pipeline map step handle field mapping.
    // evaluate only extracts the items array from the response.
    let map_code = String::new();

    let url_json = serde_json::to_string(url).unwrap_or_else(|_| format!("\"{}\"", url));
    format!(
        concat!(
            "(async () => {{\n",
            "  const res = await fetch({}, {{\n",
            "    credentials: 'include'\n",
            "  }});\n",
            "  const data = await res.json();\n",
            "  return (data{} || []){};\n",
            "}})()\n",
        ),
        url_json, path_chain, map_code
    )
}

// ── YAML pipeline generation ────────────────────────────────────────────────

/// Build the complete YAML for a candidate adapter.
fn build_candidate_yaml(
    site: &str,
    manifest: &ExploreManifest,
    cap: &SynthesizeCapability,
    endpoint: &DiscoveredEndpoint,
) -> String {
    let needs_browser = cap.strategy.requires_browser();
    let has_keyword = cap.recommended_args.iter().any(|a| a.name == "keyword");
    let templated_url = build_templated_url(&endpoint.url, has_keyword);

    let mut domain = String::new();
    if let Ok(parsed) = url::Url::parse(&manifest.url) {
        if let Some(host) = parsed.host_str() {
            domain = host.to_string();
        }
    }

    let columns = if cap.recommended_columns.is_empty() {
        vec!["title".to_string(), "url".to_string()]
    } else {
        cap.recommended_columns.clone()
    };

    // Build pipeline steps
    let mut pipeline_lines = Vec::new();

    if let (Strategy::Intercept, Some(hint)) = (cap.strategy, cap.store_hint.as_ref()) {
        // Store-action: navigate + wait + tap (declarative, clean)
        pipeline_lines.push(format!("  - navigate: \"{}\"", manifest.url));
        pipeline_lines.push("  - wait: 3".to_string());

        let mut tap_parts = vec![
            format!("      store: {}", hint.store),
            format!("      action: {}", hint.action),
            "      timeout: 8".to_string(),
        ];

        // Infer capture pattern from endpoint URL
        if let Ok(ep_url) = url::Url::parse(&endpoint.url) {
            let path_parts: Vec<&str> = ep_url
                .path_segments()
                .into_iter()
                .flatten()
                .filter(|p| !p.is_empty())
                .collect();
            let capture_part = path_parts.iter().rfind(|p| {
                let re_version = p.len() <= 3
                    && p.starts_with('v')
                    && p[1..].chars().all(|c| c.is_ascii_digit());
                !re_version
            });
            if let Some(cp) = capture_part {
                tap_parts.push(format!("      capture: {}", cp));
            }
        }
        if let Some(ref ip) = cap.item_path {
            tap_parts.push(format!("      select: {}", ip));
        }

        pipeline_lines.push("  - tap:".to_string());
        pipeline_lines.extend(tap_parts);
    } else if needs_browser {
        // Browser-based: navigate + evaluate (like bilibili/hot.yaml, twitter/trending.yaml)
        pipeline_lines.push(format!("  - navigate: \"{}\"", manifest.url));
        let item_path = cap.item_path.as_deref().unwrap_or("data");
        let detected = endpoint
            .response_analysis
            .as_ref()
            .map(|ra| &ra.detected_fields)
            .cloned()
            .unwrap_or_default();
        let eval_script = build_evaluate_script(
            &templated_url,
            item_path,
            &endpoint.fields,
            &columns,
            &detected,
        );
        pipeline_lines.push(format!(
            "  - evaluate: |\n      {}",
            eval_script.replace('\n', "\n      ")
        ));
    } else {
        // Public API: direct fetch (like hackernews/top.yaml)
        pipeline_lines.push(format!("  - fetch:\n      url: \"{}\"", templated_url));
        if let Some(ref ip) = cap.item_path {
            pipeline_lines.push(format!("  - select: \"{}\"", ip));
        }
    }

    // Map fields — use detected_fields from response_analysis for accurate paths
    let mut map_entries = Vec::new();
    if !has_keyword {
        map_entries.push("      rank: \"${{ index + 1 }}\"".to_string());
    }
    let detected = endpoint
        .response_analysis
        .as_ref()
        .map(|ra| &ra.detected_fields)
        .cloned()
        .unwrap_or_default();

    for col in &columns {
        // Priority: 1) detected_fields mapping (role → actual path)
        //           2) FieldInfo with matching role
        //           3) column name as-is
        let field_path = detected
            .get(col.as_str())
            .cloned()
            .or_else(|| {
                endpoint
                    .fields
                    .iter()
                    .find(|f| f.role.as_deref() == Some(col.as_str()))
                    .map(|f| f.name.clone())
            })
            .unwrap_or_else(|| col.clone());
        map_entries.push(format!("      {}: \"${{{{ item.{} }}}}\"", col, field_path));
    }
    pipeline_lines.push("  - map:".to_string());
    pipeline_lines.extend(map_entries);
    pipeline_lines.push("  - limit: \"${{ args.limit | default(20) }}\"".to_string());

    // Build args definition
    let args_section = build_args_section_from_recommended(&cap.recommended_args);

    // Assemble full YAML
    let all_map_keys: Vec<String> = {
        let mut keys = Vec::new();
        if !has_keyword {
            keys.push("rank".to_string());
        }
        keys.extend(columns.iter().cloned());
        keys
    };

    let mut lines = Vec::new();
    lines.push(format!("site: {}", site));
    lines.push(format!("name: {}", cap.name));
    lines.push(format!(
        "description: \"{} (auto-generated)\"",
        if cap.description.is_empty() {
            format!("{} {}", site, cap.name)
        } else {
            cap.description.clone()
        }
    ));
    if !domain.is_empty() {
        lines.push(format!("domain: {}", domain));
    }
    lines.push(format!("strategy: {}", cap.strategy));
    lines.push(format!("browser: {}", needs_browser));

    if !args_section.is_empty() {
        lines.push(String::new());
        lines.push("args:".to_string());
        lines.push(args_section);
    }

    lines.push(String::new());
    lines.push("pipeline:".to_string());
    lines.extend(pipeline_lines);

    lines.push(String::new());
    lines.push(format!(
        "columns: [{}]",
        all_map_keys
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    lines.join("\n")
}

// ── Args building ───────────────────────────────────────────────────────────

/// Build recommended args from URL query params and field analysis.
fn build_recommended_args(
    url: &str,
    _fields: &[FieldInfo],
    goal: Option<&str>,
) -> Vec<RecommendedArg> {
    let qp = extract_query_param_names(url);
    let has_search =
        qp.iter().any(|p| SEARCH_PARAMS.contains(&p.as_str())) || goal == Some("search");
    let has_pagination = qp.iter().any(|p| PAGINATION_PARAMS.contains(&p.as_str()));

    let mut args = Vec::new();

    if has_search {
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
        default: Some(serde_json::Value::Number(20.into())),
        description: Some("Number of items to return".to_string()),
    });

    if has_pagination {
        args.push(RecommendedArg {
            name: "page".to_string(),
            arg_type: "int".to_string(),
            required: false,
            default: Some(serde_json::Value::Number(1.into())),
            description: Some("Page number".to_string()),
        });
    }

    args
}

/// Build YAML args section from recommended args.
fn build_args_section_from_recommended(args: &[RecommendedArg]) -> String {
    let mut lines = Vec::new();
    for arg in args {
        lines.push(format!("  {}:", arg.name));
        lines.push(format!("    type: {}", arg.arg_type));
        if arg.required {
            lines.push("    required: true".to_string());
        }
        if let Some(ref d) = arg.default {
            match d {
                serde_json::Value::String(s) => lines.push(format!("    default: {}", s)),
                serde_json::Value::Number(n) => lines.push(format!("    default: {}", n)),
                serde_json::Value::Bool(b) => lines.push(format!("    default: {}", b)),
                other => lines.push(format!("    default: {}", other)),
            }
        }
        if let Some(ref desc) = arg.description {
            lines.push(format!("    description: {}", desc));
        }
    }
    lines.join("\n")
}

// ── Column inference ────────────────────────────────────────────────────────

/// Determine output columns from field roles.
/// Infer columns from response_analysis — uses detected_fields for accurate mapping,
/// plus sample_fields for additional useful columns.
fn infer_columns_from_analysis(
    ra: &crate::types::ResponseAnalysis,
    fields: &[FieldInfo],
) -> Vec<String> {
    let preferred_order = [
        "title", "url", "author", "score", "time", "id", "cover", "category",
    ];
    let mut cols = Vec::new();

    // First add columns from detected_fields (role-based, in preferred order)
    for &role in &preferred_order {
        if ra.detected_fields.contains_key(role) {
            cols.push(role.to_string());
        }
    }

    // If too few columns, supplement from sample_fields (actual field names)
    if cols.len() < 3 {
        let skip = ["_", "id", "mid", "uid", "cid", "oid", "rid"];
        for field in &ra.sample_fields {
            if cols.len() >= 6 {
                break;
            }
            let lower = field.to_lowercase();
            // Skip internal/id fields and already-added ones
            if skip.iter().any(|s| lower == *s) {
                continue;
            }
            if cols.iter().any(|c| c == field) {
                continue;
            }
            // Skip nested paths (keep top-level only)
            if field.contains('.') {
                continue;
            }
            cols.push(field.clone());
        }
    }

    if cols.is_empty() {
        // Ultimate fallback from FieldInfo
        return infer_columns(fields);
    }
    cols
}

fn infer_columns(fields: &[FieldInfo]) -> Vec<String> {
    let preferred_order = ["title", "url", "author", "score", "time"];
    let mut cols = Vec::new();
    for &role in &preferred_order {
        if fields.iter().any(|f| f.role.as_deref() == Some(role)) {
            cols.push(role.to_string());
        }
    }
    if cols.is_empty() {
        // Fallback
        cols.push("title".to_string());
        cols.push("url".to_string());
    }
    cols
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Detect item path from sample response by finding the deepest array.
fn detect_item_path(sample: &Option<serde_json::Value>) -> Option<String> {
    let value = sample.as_ref()?;
    find_item_path(value, "", 0)
}

fn find_item_path(value: &serde_json::Value, prefix: &str, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    if let Some(obj) = value.as_object() {
        let mut best_path: Option<String> = None;
        let mut best_len = 0;
        for (key, val) in obj {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", prefix, key)
            };
            if let Some(arr) = val.as_array() {
                if arr.len() >= 2 && arr.iter().any(|v| v.is_object()) && arr.len() > best_len {
                    best_path = Some(path.clone());
                    best_len = arr.len();
                }
            }
            // Recurse into nested objects
            if let Some(nested) = find_item_path(val, &path, depth + 1) {
                // Prefer deeper paths (more specific)
                if best_path.is_none()
                    || nested.matches('.').count()
                        > best_path
                            .as_ref()
                            .map(|p| p.matches('.').count())
                            .unwrap_or(0)
                {
                    best_path = Some(nested);
                }
            }
        }
        return best_path;
    }
    None
}

/// Detect store hint from manifest.
fn detect_store_hint(manifest: &ExploreManifest) -> Option<StoreHint> {
    manifest.store.as_ref().map(|store_name| StoreHint {
        store: store_name.clone(),
        action: "fetch".to_string(),
    })
}

fn extract_query_param_names(url: &str) -> Vec<String> {
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

fn url_last_segment(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed
        .path_segments()?
        .rfind(|s: &&str| {
            !(s.is_empty()
                || s.chars().all(|c| c.is_ascii_digit())
                || s.len() >= 8 && s.chars().all(|c| c.is_ascii_hexdigit()))
        })
        .map(|s| {
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
                .to_lowercase()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FieldInfo;
    use opencli_rs_core::Strategy;

    fn sample_manifest() -> ExploreManifest {
        ExploreManifest {
            url: "https://www.example.com/hot".to_string(),
            title: Some("Example Hot".to_string()),
            endpoints: vec![
                DiscoveredEndpoint {
                    url: "https://api.example.com/v1/hot?limit=20".to_string(),
                    method: "GET".to_string(),
                    content_type: Some("application/json".to_string()),
                    fields: vec![
                        FieldInfo {
                            name: "title".to_string(),
                            role: Some("title".to_string()),
                            field_type: "string".to_string(),
                        },
                        FieldInfo {
                            name: "url".to_string(),
                            role: Some("url".to_string()),
                            field_type: "string".to_string(),
                        },
                        FieldInfo {
                            name: "author".to_string(),
                            role: Some("author".to_string()),
                            field_type: "string".to_string(),
                        },
                    ],
                    confidence: 0.85,
                    auth_level: Strategy::Public,
                    sample_response: None,
                    pattern: String::new(),
                    query_params: vec![],
                    score: 17,
                    has_search_param: false,
                    has_pagination_param: false,
                    has_limit_param: true,
                    auth_indicators: vec![],
                    response_analysis: None,
                },
                DiscoveredEndpoint {
                    url: "https://api.example.com/v1/search?q=test&limit=20".to_string(),
                    method: "GET".to_string(),
                    content_type: Some("application/json".to_string()),
                    fields: vec![
                        FieldInfo {
                            name: "title".to_string(),
                            role: Some("title".to_string()),
                            field_type: "string".to_string(),
                        },
                        FieldInfo {
                            name: "url".to_string(),
                            role: Some("url".to_string()),
                            field_type: "string".to_string(),
                        },
                    ],
                    confidence: 0.70,
                    auth_level: Strategy::Cookie,
                    sample_response: None,
                    pattern: String::new(),
                    query_params: vec![],
                    score: 14,
                    has_search_param: true,
                    has_pagination_param: false,
                    has_limit_param: true,
                    auth_indicators: vec![],
                    response_analysis: None,
                },
            ],
            framework: Some("React".to_string()),
            store: None,
            auth_indicators: vec![],
        }
    }

    #[test]
    fn test_synthesize_returns_candidates() {
        let manifest = sample_manifest();
        let options = SynthesizeOptions::default();
        let candidates = synthesize(&manifest, options).unwrap();
        assert!(!candidates.is_empty());
        assert!(candidates.len() <= 3);
    }

    #[test]
    fn test_synthesize_candidate_has_yaml() {
        let manifest = sample_manifest();
        let options = SynthesizeOptions::default();
        let candidates = synthesize(&manifest, options).unwrap();
        for c in &candidates {
            assert!(!c.yaml.is_empty());
            assert!(c.yaml.contains("site:"));
            assert!(c.yaml.contains("pipeline:"));
        }
    }

    #[test]
    fn test_synthesize_sorted_by_confidence() {
        let manifest = sample_manifest();
        let options = SynthesizeOptions::default();
        let candidates = synthesize(&manifest, options).unwrap();
        for window in candidates.windows(2) {
            assert!(window[0].confidence >= window[1].confidence);
        }
    }

    #[test]
    fn test_synthesize_with_goal() {
        let manifest = sample_manifest();
        let options = SynthesizeOptions {
            site: None,
            goal: Some("trending".to_string()),
        };
        let candidates = synthesize(&manifest, options).unwrap();
        // All should be named "trending" (or disambiguated)
        assert!(candidates[0].name.contains("trending"));
    }

    #[test]
    fn test_build_templated_url() {
        let url = build_templated_url("https://api.example.com/data?limit=20&_=12345", false);
        assert!(url.contains("limit="));
        assert!(!url.contains("_="));
    }

    #[test]
    fn test_build_templated_url_with_keyword() {
        let url = build_templated_url("https://api.example.com/search?q=test&limit=20", true);
        assert!(url.contains("args.keyword"));
        assert!(url.contains("args.limit"));
    }

    #[test]
    fn test_infer_columns() {
        let fields = vec![
            FieldInfo {
                name: "title".into(),
                role: Some("title".into()),
                field_type: "string".into(),
            },
            FieldInfo {
                name: "score".into(),
                role: Some("score".into()),
                field_type: "number".into(),
            },
        ];
        let cols = infer_columns(&fields);
        assert_eq!(cols, vec!["title", "score"]);
    }

    #[test]
    fn test_infer_columns_fallback() {
        let fields = vec![];
        let cols = infer_columns(&fields);
        assert_eq!(cols, vec!["title", "url"]);
    }
}
