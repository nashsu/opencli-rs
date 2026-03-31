pub mod types;
pub mod explore;
pub mod synthesize;
pub mod cascade;
pub mod generate;
pub mod config;
pub mod llm;
pub mod ai_generate;
pub mod url_pattern;

pub use explore::explore;
pub use synthesize::{synthesize, render_synthesize_summary, SynthesizeCandidateSummary, SynthesizeResult};
pub use cascade::{cascade, probe_endpoint, render_cascade_result, CascadeResult};
pub use generate::{
    generate, generate_full, normalize_goal, render_generate_summary,
    GenerateOptions, GenerateResult, GenerateExploreStats, GenerateSynthesizeStats,
};
pub use types::{
    AdapterCandidate, DiscoveredEndpoint, ExploreManifest, ExploreOptions,
    ExploreResult, FieldInfo, InferredCapability, RecommendedArg, ResponseAnalysis,
    StoreHint, StoreInfo, StrategyTestResult, SynthesizeOptions,
};
pub use config::{load_config, save_config, config_path, api_base, search_url, upload_url, Config, LlmConfig};
pub use ai_generate::generate_with_ai;
pub use url_pattern::url_to_pattern;
