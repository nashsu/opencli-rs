use autocli_browser::{BrowserBridge, ReadArticle};
use autocli_core::IPage;
use htmd::HtmlToMarkdown;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Markdown,
    Text,
    Html,
    Json,
}

impl Format {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "markdown" | "md" => Some(Self::Markdown),
            "text" | "txt" | "plain" => Some(Self::Text),
            "html" => Some(Self::Html),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

fn daemon_port() -> u16 {
    std::env::var("AUTOCLI_DAEMON_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(19925)
}

pub async fn run(url: &str, format: Format, output_path: Option<&str>) -> Result<(), String> {
    let mut bridge = BrowserBridge::new(daemon_port());
    let page = bridge
        .connect_daemon_page()
        .await
        .map_err(|e| format!("{}", e))?;

    let article = page
        .read_article(url)
        .await
        .map_err(|e| format!("{}", e))?;

    // Always close the automation window when done.
    let _ = page.close().await;

    let rendered = render(&article, format)?;

    match output_path {
        Some(path) => {
            std::fs::write(Path::new(path), rendered.as_bytes())
                .map_err(|e| format!("Failed to write output file: {e}"))?;
            eprintln!("Saved to {path}");
        }
        None => {
            print!("{}", rendered);
            if !rendered.ends_with('\n') {
                println!();
            }
        }
    }
    Ok(())
}

fn render(article: &ReadArticle, format: Format) -> Result<String, String> {
    match format {
        Format::Json => serde_json::to_string_pretty(article)
            .map_err(|e| format!("Failed to serialize article: {e}")),
        Format::Html => Ok(article.content.clone()),
        Format::Text => Ok(render_text(article)),
        Format::Markdown => render_markdown(article),
    }
}

fn render_text(article: &ReadArticle) -> String {
    let mut out = String::new();
    if !article.title.is_empty() {
        out.push_str(&article.title);
        out.push_str("\n\n");
    }
    if let Some(byline) = article.byline.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(byline);
        out.push_str("\n\n");
    }
    out.push_str(article.text_content.trim());
    out
}

fn render_markdown(article: &ReadArticle) -> Result<String, String> {
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript"])
        .build();
    let body = converter
        .convert(&article.content)
        .map_err(|e| format!("HTML to Markdown conversion failed: {e}"))?;

    let mut out = String::new();
    if !article.title.is_empty() {
        out.push_str("# ");
        out.push_str(&article.title);
        out.push_str("\n\n");
    }
    // Metadata block: only include fields that are present.
    let mut meta_lines: Vec<String> = Vec::new();
    if let Some(byline) = article.byline.as_deref().filter(|s| !s.is_empty()) {
        meta_lines.push(format!("**By:** {byline}"));
    }
    if let Some(site) = article.site_name.as_deref().filter(|s| !s.is_empty()) {
        meta_lines.push(format!("**Site:** {site}"));
    }
    if let Some(published) = article.published_time.as_deref().filter(|s| !s.is_empty()) {
        meta_lines.push(format!("**Published:** {published}"));
    }
    if !article.url.is_empty() {
        meta_lines.push(format!("**URL:** <{}>", article.url));
    }
    if !meta_lines.is_empty() {
        out.push_str(&meta_lines.join("  \n"));
        out.push_str("\n\n---\n\n");
    }

    out.push_str(body.trim());
    out.push('\n');
    Ok(out)
}
