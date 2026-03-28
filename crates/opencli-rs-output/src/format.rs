use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// Supported output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Yaml,
    Csv,
    Markdown,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table => write!(f, "table"),
            Self::Json => write!(f, "json"),
            Self::Yaml => write!(f, "yaml"),
            Self::Csv => write!(f, "csv"),
            Self::Markdown => write!(f, "markdown"),
        }
    }
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "yaml" => Ok(Self::Yaml),
            "csv" => Ok(Self::Csv),
            "md" | "markdown" => Ok(Self::Markdown),
            other => Err(format!("unknown output format: '{other}'")),
        }
    }
}

/// Options controlling how output is rendered.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub format: OutputFormat,
    pub columns: Option<Vec<String>>,
    pub title: Option<String>,
    pub elapsed: Option<Duration>,
    pub source: Option<String>,
    pub footer_extra: Option<String>,
}
