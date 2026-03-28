use std::collections::HashMap;

use opencli_rs_core::Strategy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── ExploreOptions ───────────────────────────────────────────────────────────

/// Options for the explore command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExploreOptions {
    /// Timeout in seconds (default 120).
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Maximum number of auto-scrolls (default 5).
    #[serde(default)]
    pub max_scrolls: Option<u32>,
    /// Whether to capture network traffic (default true).
    #[serde(default)]
    pub capture_network: Option<bool>,
    /// Seconds to wait after navigation before capturing (default 3).
    #[serde(default)]
    pub wait_seconds: Option<f64>,
    /// Whether to perform auto-fuzzing (click interactive elements).
    #[serde(default)]
    pub auto_fuzz: Option<bool>,
    /// Button/tab labels to click during fuzzing.
    #[serde(default)]
    pub click_labels: Vec<String>,
    /// The exploration goal (e.g. "hot", "search").
    #[serde(default)]
    pub goal: Option<String>,
    /// Override site name instead of auto-detecting.
    #[serde(default)]
    pub site_name: Option<String>,
}

impl Default for ExploreOptions {
    fn default() -> Self {
        Self {
            timeout: Some(120),
            max_scrolls: Some(5),
            capture_network: Some(true),
            wait_seconds: Some(3.0),
            auto_fuzz: None,
            click_labels: vec![],
            goal: None,
            site_name: None,
        }
    }
}

// ── Response analysis ────────────────────────────────────────────────────────

/// Analysis of a JSON response body: the item array path, count, and detected fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseAnalysis {
    /// JSON path to the item array (e.g. "data.list").
    pub item_path: Option<String>,
    /// Number of items found in the array.
    pub item_count: usize,
    /// Detected field roles (role -> field name).
    pub detected_fields: HashMap<String, String>,
    /// All top-level sample field names from the first item.
    pub sample_fields: Vec<String>,
}

// ── DiscoveredEndpoint ───────────────────────────────────────────────────────

/// A single discovered API endpoint with analysis metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredEndpoint {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub content_type: Option<String>,
    /// Fields extracted from the response body.
    pub fields: Vec<FieldInfo>,
    /// Confidence score from 0.0 to 1.0 (derived from score).
    pub confidence: f64,
    pub auth_level: Strategy,
    #[serde(default)]
    pub sample_response: Option<Value>,

    // ── New fields matching the TS AnalyzedEndpoint ──
    /// Normalized URL pattern (host + path with {id}/{hex} replacements).
    #[serde(default)]
    pub pattern: String,
    /// Non-volatile query parameter names.
    #[serde(default)]
    pub query_params: Vec<String>,
    /// Raw integer score before normalization (JSON +10, array +5, etc.).
    #[serde(default)]
    pub score: i32,
    /// Whether any search-related query parameter is present.
    #[serde(default)]
    pub has_search_param: bool,
    /// Whether any pagination-related query parameter is present.
    #[serde(default)]
    pub has_pagination_param: bool,
    /// Whether any limit-related query parameter is present.
    #[serde(default)]
    pub has_limit_param: bool,
    /// Auth indicators found in request headers (e.g. "bearer", "csrf", "signature").
    #[serde(default)]
    pub auth_indicators: Vec<String>,
    /// Structured analysis of the response body.
    #[serde(default)]
    pub response_analysis: Option<ResponseAnalysis>,
}

// ── StoreInfo ────────────────────────────────────────────────────────────────

/// A discovered Pinia or Vuex store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreInfo {
    /// Store type: "pinia" or "vuex".
    #[serde(rename = "type")]
    pub store_type: String,
    /// Store identifier (module name or pinia store id).
    pub id: String,
    /// Public action/method names.
    pub actions: Vec<String>,
    /// Top-level state key names.
    #[serde(default)]
    pub state_keys: Vec<String>,
}

// ── InferredCapability ───────────────────────────────────────────────────────

/// A CLI capability inferred from a discovered endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferredCapability {
    pub name: String,
    pub description: String,
    /// Recommended strategy: "cookie", "header", "intercept", "store-action", "public".
    pub strategy: String,
    /// Confidence from 0.0 to 1.0.
    pub confidence: f64,
    /// The endpoint pattern this capability maps to.
    pub endpoint: String,
    /// JSON path to the item array in the response.
    pub item_path: Option<String>,
    /// Recommended output columns (e.g. ["title", "url", "author"]).
    pub recommended_columns: Vec<String>,
    /// Recommended CLI arguments.
    pub recommended_args: Vec<RecommendedArg>,
    /// Optional store hint for store-action strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_hint: Option<StoreHint>,
}

/// A recommended CLI argument for a capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedArg {
    pub name: String,
    #[serde(rename = "type")]
    pub arg_type: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Hint for invoking a Pinia/Vuex store action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreHint {
    pub store: String,
    pub action: String,
}

// ── ExploreManifest ──────────────────────────────────────────────────────────

/// The manifest produced by an explore run — summarizes the site's API surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExploreManifest {
    pub url: String,
    pub title: Option<String>,
    pub endpoints: Vec<DiscoveredEndpoint>,
    /// Detected frontend framework (React, Vue, Angular, etc.).
    pub framework: Option<String>,
    /// Detected state store (Pinia, Vuex, Redux, etc.).
    pub store: Option<String>,
    /// Auth-related indicators found in request headers/cookies.
    pub auth_indicators: Vec<String>,
}

// ── ExploreResult ────────────────────────────────────────────────────────────

/// Full result of an explore run, including inferred capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExploreResult {
    pub site: String,
    pub target_url: String,
    pub final_url: String,
    pub title: String,
    pub framework: HashMap<String, bool>,
    pub stores: Vec<StoreInfo>,
    pub top_strategy: String,
    pub endpoint_count: usize,
    pub api_endpoint_count: usize,
    pub capabilities: Vec<InferredCapability>,
    pub auth_indicators: Vec<String>,
    pub out_dir: String,
}

// ── FieldInfo ────────────────────────────────────────────────────────────────

/// A field discovered within an API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    /// Semantic role: title, url, author, score, date, etc.
    pub role: Option<String>,
    /// Type: string, number, boolean, array.
    pub field_type: String,
}

// ── Adapter / Synthesize / Cascade types ─────────────────────────────────────

/// A candidate adapter generated by synthesize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterCandidate {
    pub site: String,
    pub name: String,
    pub description: String,
    pub strategy: Strategy,
    /// Generated YAML content for the adapter.
    pub yaml: String,
    pub confidence: f64,
}

/// Options for the synthesize command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SynthesizeOptions {
    pub site: Option<String>,
    pub goal: Option<String>,
}

/// Result of the cascade auth-strategy probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CascadeResult {
    pub url: String,
    pub strategy: Strategy,
    pub tested: Vec<StrategyTestResult>,
}

/// Outcome of testing a single strategy against an endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyTestResult {
    pub strategy: Strategy,
    pub success: bool,
    pub status_code: Option<u16>,
    pub has_data: bool,
}

// ── Constants ────────────────────────────────────────────────────────────────

/// URL parameters that should be ignored when normalizing endpoints (volatile/tracking).
pub(crate) const VOLATILE_PARAMS: &[&str] = &[
    "_",
    "t",
    "ts",
    "timestamp",
    "cb",
    "callback",
    "nonce",
    "rand",
    "random",
    "spm_id_from",
    "vd_source",
    "from_spmid",
    "seid",
    "rt",
    "mid",
    "web_location",
    "platform",
    "w_rid",
    "wts",
    "sign",
];

/// Parameters that indicate search capability.
pub(crate) const SEARCH_PARAMS: &[&str] = &[
    "q",
    "query",
    "keyword",
    "keywords",
    "search",
    "search_query",
    "w",
    "wd",
    "kw",
];

/// Parameters that indicate pagination.
pub(crate) const PAGINATION_PARAMS: &[&str] = &[
    "page", "pn", "p", "offset", "cursor", "next", "page_num", "pageNum",
];

/// Parameters that indicate limit/page-size.
pub(crate) const LIMIT_PARAMS: &[&str] = &[
    "limit",
    "ps",
    "size",
    "pageSize",
    "page_size",
    "count",
    "num",
    "per_page",
];

/// Well-known field roles and their common aliases.
pub(crate) const FIELD_ROLES: &[(&str, &[&str])] = &[
    (
        "title",
        &[
            "title",
            "name",
            "text",
            "content",
            "desc",
            "description",
            "headline",
            "subject",
        ],
    ),
    (
        "url",
        &[
            "url",
            "uri",
            "link",
            "href",
            "permalink",
            "jump_url",
            "web_url",
            "share_url",
        ],
    ),
    (
        "author",
        &[
            "author",
            "username",
            "user_name",
            "nickname",
            "nick",
            "owner",
            "creator",
            "up_name",
            "uname",
        ],
    ),
    (
        "score",
        &[
            "score",
            "hot",
            "heat",
            "likes",
            "like_count",
            "view_count",
            "views",
            "play",
            "favorite_count",
            "reply_count",
        ],
    ),
    (
        "time",
        &[
            "time",
            "created_at",
            "publish_time",
            "pub_time",
            "date",
            "ctime",
            "mtime",
            "pubdate",
            "created",
        ],
    ),
    (
        "id",
        &[
            "id", "aid", "bvid", "mid", "uid", "oid", "note_id", "item_id",
        ],
    ),
    (
        "cover",
        &["cover", "pic", "image", "thumbnail", "poster", "avatar"],
    ),
    (
        "category",
        &["category", "tag", "type", "tname", "channel", "section"],
    ),
];

/// Known site hostname aliases.
pub(crate) const KNOWN_SITE_ALIASES: &[(&str, &str)] = &[
    ("x.com", "twitter"),
    ("twitter.com", "twitter"),
    ("news.ycombinator.com", "hackernews"),
    ("www.zhihu.com", "zhihu"),
    ("www.bilibili.com", "bilibili"),
    ("search.bilibili.com", "bilibili"),
    ("www.v2ex.com", "v2ex"),
    ("www.reddit.com", "reddit"),
    ("www.xiaohongshu.com", "xiaohongshu"),
    ("www.douban.com", "douban"),
    ("www.weibo.com", "weibo"),
    ("www.bbc.com", "bbc"),
];
