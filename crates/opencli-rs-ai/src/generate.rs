//! One-shot generation: explore a URL and synthesize the best adapter candidate.
//!
//! Orchestrates the full pipeline:
//!   explore (Deep Explore) -> synthesize (YAML generation) -> select best candidate
//!
//! Includes goal normalization with alias table supporting both English and Chinese.

use opencli_rs_core::{CliError, IPage};

use crate::explore::explore;
use crate::synthesize::{synthesize, SynthesizeCandidateSummary};
use crate::types::{AdapterCandidate, ExploreOptions, SynthesizeOptions};

// ── Capability alias table ──────────────────────────────────────────────────

/// Alias table for normalizing goals to canonical capability names.
/// Supports English and Chinese terms.
/// Uses a Vec to preserve insertion order (matching TS object iteration order).
fn capability_aliases() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("search", vec!["search", "搜索", "查找", "query", "keyword"]),
        (
            "hot",
            vec!["hot", "热门", "热榜", "热搜", "popular", "top", "ranking"],
        ),
        ("trending", vec!["trending", "趋势", "流行", "discover"]),
        (
            "feed",
            vec!["feed", "动态", "关注", "时间线", "timeline", "following"],
        ),
        ("me", vec!["profile", "me", "个人信息", "myinfo", "账号"]),
        ("detail", vec!["detail", "详情", "video", "article", "view"]),
        ("comments", vec!["comments", "评论", "回复", "reply"]),
        ("history", vec!["history", "历史", "记录"]),
        ("favorite", vec!["favorite", "收藏", "bookmark", "collect"]),
    ]
}

/// Normalize a goal string to a standard capability name using the alias table.
pub fn normalize_goal(goal: Option<&str>) -> Option<String> {
    let goal = goal?;
    let lower = goal.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }

    for (cap, aliases) in &capability_aliases() {
        if lower == *cap || aliases.iter().any(|a| lower.contains(&a.to_lowercase())) {
            return Some(cap.to_string());
        }
    }

    None
}

// ── Result types ────────────────────────────────────────────────────────────

/// Options for the generate command.
pub struct GenerateOptions {
    pub url: String,
    pub goal: Option<String>,
    pub site: Option<String>,
    pub top: Option<usize>,
}

/// Result of the generate command.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenerateResult {
    pub ok: bool,
    pub goal: Option<String>,
    pub normalized_goal: Option<String>,
    pub site: String,
    pub selected_candidate: Option<SynthesizeCandidateSummary>,
    pub selected_command: String,
    pub explore: GenerateExploreStats,
    pub synthesize: GenerateSynthesizeStats,
}

/// Explore statistics included in the generate result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenerateExploreStats {
    pub endpoint_count: usize,
    pub api_endpoint_count: usize,
    pub capability_count: usize,
    pub top_strategy: String,
    pub framework: Option<String>,
}

/// Synthesize statistics included in the generate result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenerateSynthesizeStats {
    pub candidate_count: usize,
    pub candidates: Vec<SynthesizeCandidateSummary>,
}

// ── Candidate selection ─────────────────────────────────────────────────────

/// Select the best candidate matching the user's goal.
fn select_candidate<'a>(
    candidates: &'a [AdapterCandidate],
    goal: Option<&str>,
) -> Option<&'a AdapterCandidate> {
    if candidates.is_empty() {
        return None;
    }
    // No goal: return highest confidence first (already sorted)
    if goal.is_none() {
        return candidates.first();
    }

    let goal_str = goal.unwrap();
    let normalized = normalize_goal(Some(goal_str));

    // Try exact match on normalized goal
    if let Some(ref norm) = normalized {
        let exact = candidates.iter().find(|c| c.name == *norm);
        if exact.is_some() {
            return exact;
        }
    }

    // Try partial match
    let lower = goal_str.trim().to_lowercase();
    let partial = candidates
        .iter()
        .find(|c| c.name.to_lowercase().contains(&lower) || lower.contains(&c.name.to_lowercase()));

    partial.or_else(|| candidates.first())
}

// ── Public API ──────────────────────────────────────────────────────────────

/// One-shot generation: explore a URL, synthesize candidates, and return the best one.
///
/// 1. Call explore() with the target URL
/// 2. Call synthesize() with the explore output
/// 3. Normalize user's goal using alias table
/// 4. Match goal against generated capabilities, select best match
/// 5. If no goal, select highest confidence candidate
/// 6. Return structured result with explore stats + selected candidate
pub async fn generate(
    page: &dyn IPage,
    url: &str,
    goal: &str,
) -> Result<AdapterCandidate, CliError> {
    // Step 1: Deep Explore
    let manifest = explore(page, url, ExploreOptions::default()).await?;

    if manifest.endpoints.is_empty() {
        return Err(CliError::empty_result(format!(
            "No API endpoints discovered at {}",
            url
        )));
    }

    // Step 2: Normalize goal
    let normalized = normalize_goal(if goal.is_empty() { None } else { Some(goal) });
    let effective_goal = normalized.as_deref().unwrap_or(goal);

    // Step 3: Synthesize candidates
    let options = SynthesizeOptions {
        site: None,
        goal: Some(effective_goal.to_string()),
    };
    let candidates = synthesize(&manifest, options)?;

    // Step 4: Select best candidate for goal
    let goal_for_select = if goal.is_empty() { None } else { Some(goal) };
    let selected = select_candidate(&candidates, goal_for_select);

    selected.cloned().ok_or_else(|| {
        CliError::empty_result(format!(
            "Could not generate adapter for {} with goal '{}'",
            url, goal
        ))
    })
}

/// Full generate with structured result (for programmatic use).
pub async fn generate_full(
    page: &dyn IPage,
    opts: GenerateOptions,
) -> Result<GenerateResult, CliError> {
    // Step 1: Deep Explore
    let manifest = explore(page, &opts.url, ExploreOptions::default()).await?;

    let endpoint_count = manifest.endpoints.len();
    let api_endpoint_count = manifest
        .endpoints
        .iter()
        .filter(|e| {
            e.content_type
                .as_deref()
                .map(|ct| ct.contains("json"))
                .unwrap_or(false)
        })
        .count();

    // Step 2: Normalize goal
    let normalized_goal = normalize_goal(opts.goal.as_deref());
    let effective_goal = normalized_goal.as_deref().or(opts.goal.as_deref());

    // Step 3: Synthesize candidates
    let synth_options = SynthesizeOptions {
        site: opts.site.clone(),
        goal: effective_goal.map(String::from),
    };
    let candidates = synthesize(&manifest, synth_options)?;

    let candidate_count = candidates.len();

    // Step 4: Select best candidate
    let selected = select_candidate(&candidates, opts.goal.as_deref());

    let site = crate::explore::detect_site_name(&opts.url);
    let selected_summary = selected.map(|c| SynthesizeCandidateSummary {
        name: c.name.clone(),
        strategy: c.strategy.to_string(),
        confidence: c.confidence,
    });
    let selected_command = selected
        .map(|c| format!("{}/{}", site, c.name))
        .unwrap_or_else(|| "(none)".to_string());

    // Infer top strategy from endpoints
    let top_strategy = manifest
        .endpoints
        .first()
        .map(|e| e.auth_level.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let ok = endpoint_count > 0 && candidate_count > 0;

    Ok(GenerateResult {
        ok,
        goal: opts.goal,
        normalized_goal: normalized_goal.clone(),
        site,
        selected_candidate: selected_summary,
        selected_command,
        explore: GenerateExploreStats {
            endpoint_count,
            api_endpoint_count,
            capability_count: candidate_count,
            top_strategy,
            framework: manifest.framework,
        },
        synthesize: GenerateSynthesizeStats {
            candidate_count,
            candidates: candidates
                .iter()
                .map(|c| SynthesizeCandidateSummary {
                    name: c.name.clone(),
                    strategy: c.strategy.to_string(),
                    confidence: c.confidence,
                })
                .collect(),
        },
    })
}

/// Render a human-readable summary of the generate result.
pub fn render_generate_summary(r: &GenerateResult) -> String {
    let mut lines = vec![
        format!("opencli generate: {}", if r.ok { "OK" } else { "FAIL" }),
        format!("Site: {}", r.site),
        format!("Goal: {}", r.goal.as_deref().unwrap_or("(auto)")),
        format!("Selected: {}", r.selected_command),
        String::new(),
        "Explore:".to_string(),
        format!(
            "  Endpoints: {} total, {} API",
            r.explore.endpoint_count, r.explore.api_endpoint_count
        ),
        format!("  Capabilities: {}", r.explore.capability_count),
        format!("  Strategy: {}", r.explore.top_strategy),
        String::new(),
        "Synthesize:".to_string(),
        format!("  Candidates: {}", r.synthesize.candidate_count),
    ];

    for c in &r.synthesize.candidates {
        lines.push(format!(
            "    - {} ({}, {:.0}%)",
            c.name,
            c.strategy,
            c.confidence * 100.0,
        ));
    }

    if let Some(ref fw) = r.explore.framework {
        lines.push(format!("Framework: {}", fw));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_goal_english() {
        assert_eq!(normalize_goal(Some("search")), Some("search".to_string()));
        assert_eq!(normalize_goal(Some("hot")), Some("hot".to_string()));
        assert_eq!(
            normalize_goal(Some("trending")),
            Some("trending".to_string())
        );
        assert_eq!(normalize_goal(Some("popular")), Some("hot".to_string()));
        assert_eq!(normalize_goal(Some("top")), Some("hot".to_string()));
        assert_eq!(normalize_goal(Some("ranking")), Some("hot".to_string()));
        assert_eq!(normalize_goal(Some("timeline")), Some("feed".to_string()));
        assert_eq!(normalize_goal(Some("profile")), Some("me".to_string()));
    }

    #[test]
    fn test_normalize_goal_chinese() {
        assert_eq!(normalize_goal(Some("搜索")), Some("search".to_string()));
        assert_eq!(normalize_goal(Some("热门")), Some("hot".to_string()));
        assert_eq!(normalize_goal(Some("热榜")), Some("hot".to_string()));
        assert_eq!(normalize_goal(Some("趋势")), Some("trending".to_string()));
        assert_eq!(normalize_goal(Some("动态")), Some("feed".to_string()));
        assert_eq!(normalize_goal(Some("个人信息")), Some("me".to_string()));
        assert_eq!(normalize_goal(Some("评论")), Some("comments".to_string()));
        assert_eq!(normalize_goal(Some("收藏")), Some("favorite".to_string()));
    }

    #[test]
    fn test_normalize_goal_none() {
        assert_eq!(normalize_goal(None), None);
        assert_eq!(normalize_goal(Some("")), None);
    }

    #[test]
    fn test_normalize_goal_unknown() {
        assert_eq!(normalize_goal(Some("xyzzy")), None);
    }

    #[test]
    fn test_select_candidate_no_goal() {
        let candidates = vec![
            AdapterCandidate {
                site: "test".into(),
                name: "hot".into(),
                description: "Hot".into(),
                strategy: opencli_rs_core::Strategy::Public,
                yaml: String::new(),
                confidence: 0.9,
            },
            AdapterCandidate {
                site: "test".into(),
                name: "search".into(),
                description: "Search".into(),
                strategy: opencli_rs_core::Strategy::Cookie,
                yaml: String::new(),
                confidence: 0.7,
            },
        ];
        let selected = select_candidate(&candidates, None);
        assert_eq!(selected.unwrap().name, "hot");
    }

    #[test]
    fn test_select_candidate_with_goal() {
        let candidates = vec![
            AdapterCandidate {
                site: "test".into(),
                name: "hot".into(),
                description: "Hot".into(),
                strategy: opencli_rs_core::Strategy::Public,
                yaml: String::new(),
                confidence: 0.9,
            },
            AdapterCandidate {
                site: "test".into(),
                name: "search".into(),
                description: "Search".into(),
                strategy: opencli_rs_core::Strategy::Cookie,
                yaml: String::new(),
                confidence: 0.7,
            },
        ];
        let selected = select_candidate(&candidates, Some("search"));
        assert_eq!(selected.unwrap().name, "search");
    }

    #[test]
    fn test_select_candidate_with_chinese_goal() {
        let candidates = vec![
            AdapterCandidate {
                site: "test".into(),
                name: "hot".into(),
                description: "Hot".into(),
                strategy: opencli_rs_core::Strategy::Public,
                yaml: String::new(),
                confidence: 0.9,
            },
            AdapterCandidate {
                site: "test".into(),
                name: "search".into(),
                description: "Search".into(),
                strategy: opencli_rs_core::Strategy::Cookie,
                yaml: String::new(),
                confidence: 0.7,
            },
        ];
        // "搜索" normalizes to "search"
        let selected = select_candidate(&candidates, Some("搜索"));
        assert_eq!(selected.unwrap().name, "search");
    }

    #[test]
    fn test_generate_module_compiles() {
        assert_eq!(
            std::mem::size_of::<crate::types::AdapterCandidate>(),
            std::mem::size_of::<crate::types::AdapterCandidate>()
        );
    }

    #[test]
    fn test_render_generate_summary() {
        let result = GenerateResult {
            ok: true,
            goal: Some("hot".to_string()),
            normalized_goal: Some("hot".to_string()),
            site: "example".to_string(),
            selected_candidate: Some(SynthesizeCandidateSummary {
                name: "hot".to_string(),
                strategy: "public".to_string(),
                confidence: 0.85,
            }),
            selected_command: "example/hot".to_string(),
            explore: GenerateExploreStats {
                endpoint_count: 3,
                api_endpoint_count: 2,
                capability_count: 2,
                top_strategy: "public".to_string(),
                framework: Some("React".to_string()),
            },
            synthesize: GenerateSynthesizeStats {
                candidate_count: 2,
                candidates: vec![SynthesizeCandidateSummary {
                    name: "hot".to_string(),
                    strategy: "public".to_string(),
                    confidence: 0.85,
                }],
            },
        };
        let rendered = render_generate_summary(&result);
        assert!(rendered.contains("opencli generate: OK"));
        assert!(rendered.contains("Site: example"));
        assert!(rendered.contains("hot"));
    }
}
