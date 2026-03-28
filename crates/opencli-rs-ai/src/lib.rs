pub mod cascade;
pub mod explore;
pub mod generate;
pub mod synthesize;
pub mod types;

pub use cascade::{cascade, probe_endpoint, render_cascade_result, CascadeResult};
pub use explore::explore;
pub use generate::{
    generate, generate_full, normalize_goal, render_generate_summary, GenerateExploreStats,
    GenerateOptions, GenerateResult, GenerateSynthesizeStats,
};
pub use synthesize::{
    render_synthesize_summary, synthesize, SynthesizeCandidateSummary, SynthesizeResult,
};
pub use types::{
    AdapterCandidate, DiscoveredEndpoint, ExploreManifest, ExploreOptions, ExploreResult,
    FieldInfo, InferredCapability, RecommendedArg, ResponseAnalysis, StoreHint, StoreInfo,
    StrategyTestResult, SynthesizeOptions,
};
