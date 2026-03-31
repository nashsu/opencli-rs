use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;
use tracing::{debug, info};

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template_str, TemplateContext};

// ---------------------------------------------------------------------------
// DownloadStep
// ---------------------------------------------------------------------------

/// DownloadStep handles media/article downloads.
/// Supports:
/// - `tool: yt-dlp` — invoke yt-dlp for video downloads
/// - `type: media` — download media files directly
/// - `type: article` — extract article content
pub struct DownloadStep;

#[async_trait]
impl StepHandler for DownloadStep {
    fn name(&self) -> &'static str {
        "download"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let obj = params.as_object();

        let tool = obj
            .and_then(|o| o.get("tool"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let ctx = TemplateContext {
            args: args.clone(),
            data: data.clone(),
            item: Value::Null,
            index: 0,
        };

        if tool == "yt-dlp" {
            return execute_ytdlp(obj.unwrap_or(&serde_json::Map::new()), &ctx, data).await;
        }

        let download_type = obj
            .and_then(|o| o.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("media");

        if download_type == "article" {
            return execute_article_download(obj.unwrap_or(&serde_json::Map::new()), &ctx, data).await;
        }

        if download_type == "twitter-media" || download_type == "media-batch" {
            return execute_media_batch_download(obj.unwrap_or(&serde_json::Map::new()), &ctx, data).await;
        }

        // Default: metadata-only download (extract URLs and annotate)

        let url = obj
            .and_then(|o| o.get("url"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| data.get("url").and_then(|v| v.as_str()).map(String::from));

        let mut result = match data {
            Value::Object(obj) => obj.clone(),
            _ => serde_json::Map::new(),
        };

        result.insert("download_type".to_string(), Value::String(download_type.to_string()));
        if let Some(ref u) = url {
            let filename = u.rsplit('/').next().unwrap_or("download")
                .split('?').next().unwrap_or("download");
            result.insert("download_url".to_string(), Value::String(u.clone()));
            result.insert("download_path".to_string(), Value::String(filename.to_string()));
        }
        result.insert("download_status".to_string(), Value::String("pending".to_string()));

        Ok(Value::Object(result))
    }
}

/// Execute article download — save markdown content to file with image localization
async fn execute_article_download(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    let title = params.get("title")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .or_else(|| data.get("title").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "article".to_string());

    let output_dir = params.get("output")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .unwrap_or_else(|| "./articles".to_string());

    let filename = params.get("filename")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .or_else(|| data.get("filename").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "article.md".to_string());

    let mut content = params.get("content")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .or_else(|| data.get("content").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_default();

    if content.is_empty() {
        return Ok(serde_json::json!([{
            "title": title,
            "author": "-",
            "status": "failed",
            "size": "No content to save"
        }]));
    }

    // Create article directory (output/safe_title/)
    let safe_title: String = title.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect::<String>().trim().chars().take(80).collect();
    let article_dir = std::path::PathBuf::from(output_dir).join(&safe_title).to_string_lossy().to_string();
    let _ = std::fs::create_dir_all(&article_dir);

    // Download images if present in data
    let image_urls: Vec<String> = data.get("imageUrls")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    if !image_urls.is_empty() {
        let images_dir = std::path::PathBuf::from(&article_dir).join("images").to_string_lossy().to_string();
        let _ = std::fs::create_dir_all(&images_dir);

        let referer = data.get("referer").and_then(|v| v.as_str()).unwrap_or("");
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        let mut seen = std::collections::HashSet::new();
        let mut img_index = 0;

        for raw_url in &image_urls {
            if seen.contains(raw_url.as_str()) { continue; }
            seen.insert(raw_url.as_str());
            img_index += 1;

            let mut img_url = raw_url.clone();
            if img_url.starts_with("//") { img_url = format!("https:{}", img_url); }

            // Detect extension
            let ext = if let Some(m) = img_url.find("wx_fmt=") {
                img_url[m + 7..].split(&['&', '?', ' '][..]).next().unwrap_or("png").to_string()
            } else {
                img_url.rsplit('.').next()
                    .and_then(|e| e.split(&['?', '#', '&'][..]).next())
                    .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_alphanumeric()))
                    .unwrap_or("jpg").to_string()
            };

            let img_filename = format!("img_{:03}.{}", img_index, ext);
            let img_path = std::path::PathBuf::from(&images_dir).join(&img_filename).to_string_lossy().to_string();
            let local_path = format!("images/{}", img_filename);

            let mut req = client.get(&img_url);
            if !referer.is_empty() {
                req = req.header("Referer", referer);
            }

            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(bytes) = resp.bytes().await {
                        if std::fs::write(&img_path, &bytes).is_ok() {
                            debug!(img = %img_filename, size = bytes.len(), "Image downloaded");
                            // Replace remote URL with local path in markdown
                            content = content.replace(raw_url.as_str(), &local_path);
                        }
                    }
                }
                _ => {
                    debug!(url = %img_url, "Image download failed, keeping remote URL");
                }
            }
        }
        info!(count = img_index, "Images downloaded");
    }

    // Write markdown file
    let file_path = std::path::PathBuf::from(&article_dir).join(&filename).to_string_lossy().to_string();
    match std::fs::write(&file_path, &content) {
        Ok(_) => {
            let size = content.len();
            let size_str = if size > 1_000_000 { format!("{:.1} MB", size as f64 / 1e6) }
                else if size > 1000 { format!("{:.1} KB", size as f64 / 1e3) }
                else { format!("{} bytes", size) };

            info!(title = %title, path = %file_path, size = %size_str, "Article saved");
            let author = data.get("author").and_then(|v| v.as_str()).unwrap_or("-");

            Ok(serde_json::json!([{
                "title": title,
                "author": author,
                "status": "ok",
                "size": size_str,
                "path": file_path,
                "images": image_urls.len(),
            }]))
        }
        Err(e) => {
            Ok(serde_json::json!([{
                "title": title,
                "author": "-",
                "status": "failed",
                "size": format!("Write error: {}", e)
            }]))
        }
    }
}

/// Execute batch media download (images + videos from a list)
async fn execute_media_batch_download(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    let output_dir = params.get("output")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .unwrap_or_else(|| "./downloads".to_string());

    let prefix = params.get("username")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .unwrap_or_else(|| "media".to_string());

    let items = data.get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if items.is_empty() {
        // Data might be the error array itself
        if data.is_array() {
            return Ok(data.clone());
        }
        return Ok(serde_json::json!([{ "index": 0, "type": "-", "status": "failed", "size": "No media items" }]));
    }

    let _ = std::fs::create_dir_all(&output_dir);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let mut results = Vec::new();

    for (i, item) in items.iter().enumerate() {
        let media_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
        let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");

        if url.is_empty() { continue; }

        let idx = i + 1;

        if media_type == "image" {
            // Direct image download
            let ext = if url.contains("format=png") { "png" }
                else if url.contains("format=webp") { "webp" }
                else { "jpg" };
            let filename = format!("{}_{:03}.{}", prefix, idx, ext);
            let filepath = std::path::PathBuf::from(&output_dir).join(&filename).to_string_lossy().to_string();

            match client.get(url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.bytes().await {
                        Ok(bytes) => {
                            if std::fs::write(&filepath, &bytes).is_ok() {
                                let size = format_size(bytes.len());
                                results.push(serde_json::json!({
                                    "index": idx, "type": "image", "status": "ok", "size": size
                                }));
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            results.push(serde_json::json!({
                "index": idx, "type": "image", "status": "failed", "size": "-"
            }));
        } else if media_type == "video" {
            // Direct video download
            let filename = format!("{}_{:03}.mp4", prefix, idx);
            let filepath = std::path::PathBuf::from(&output_dir).join(&filename).to_string_lossy().to_string();

            match client.get(url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.bytes().await {
                        Ok(bytes) => {
                            if std::fs::write(&filepath, &bytes).is_ok() {
                                let size = format_size(bytes.len());
                                results.push(serde_json::json!({
                                    "index": idx, "type": "video", "status": "ok", "size": size
                                }));
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            results.push(serde_json::json!({
                "index": idx, "type": "video", "status": "failed", "size": "-"
            }));
        } else if media_type == "video-tweet" {
            // Use yt-dlp for tweet videos
            let filename = format!("{}_{:03}.mp4", prefix, idx);
            let filepath = std::path::PathBuf::from(&output_dir).join(&filename).to_string_lossy().to_string();

            let status = tokio::process::Command::new("yt-dlp")
                .args(["-f", "best[ext=mp4]/best", "--merge-output-format", "mp4", "-o", &filepath, url])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;

            match status {
                Ok(s) if s.success() => {
                    let size = std::fs::metadata(&filepath)
                        .map(|m| format_size(m.len() as usize))
                        .unwrap_or("-".to_string());
                    results.push(serde_json::json!({
                        "index": idx, "type": "video", "status": "ok", "size": size
                    }));
                }
                _ => {
                    results.push(serde_json::json!({
                        "index": idx, "type": "video", "status": "failed (yt-dlp)", "size": "-"
                    }));
                }
            }
        }
    }

    if results.is_empty() {
        return Ok(serde_json::json!([{ "index": 0, "type": "-", "status": "no media", "size": "-" }]));
    }

    info!(count = results.len(), dir = %output_dir, "Media batch download complete");
    Ok(Value::Array(results))
}

fn format_size(bytes: usize) -> String {
    if bytes > 1_000_000_000 { format!("{:.1} GB", bytes as f64 / 1e9) }
    else if bytes > 1_000_000 { format!("{:.1} MB", bytes as f64 / 1e6) }
    else if bytes > 1000 { format!("{:.1} KB", bytes as f64 / 1e3) }
    else { format!("{} bytes", bytes) }
}

/// Execute yt-dlp download
async fn execute_ytdlp(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    // Check if yt-dlp is installed
    let ytdlp_ok = tokio::process::Command::new(if cfg!(target_os = "windows") { "where" } else { "which" })
        .arg("yt-dlp")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    if !ytdlp_ok {
        return Ok(serde_json::json!([{
            "status": "failed",
            "size": "yt-dlp not installed. Run: pip install yt-dlp"
        }]));
    }

    // Render template params
    let url = params.get("url")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .or_else(|| data.get("url").and_then(|v| v.as_str()).map(String::from))
        .ok_or_else(|| CliError::pipeline("download: missing url"))?;

    let title = params.get("title")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .or_else(|| data.get("title").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "video".to_string());

    let output_dir = params.get("output")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .unwrap_or_else(|| "./downloads".to_string());

    let quality = params.get("quality")
        .and_then(|v| v.as_str())
        .map(|s| render_template_str(s, ctx).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| s.to_string()))
        .unwrap_or_else(|| "best".to_string());

    // Extract cookies from data (set by evaluate step from document.cookie)
    let cookies_str = data.get("cookies")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Create output directory
    let _ = std::fs::create_dir_all(&output_dir);

    // Sanitize title for filename
    let safe_title: String = title.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' { c } else { '_' })
        .collect::<String>()
        .trim()
        .chars()
        .take(100)
        .collect();

    // Build yt-dlp format string
    let format = match quality.as_str() {
        "1080p" => "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080]",
        "720p" => "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720]",
        "480p" => "bestvideo[height<=480][ext=mp4]+bestaudio[ext=m4a]/best[height<=480]",
        _ => "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
    };

    let output_path = std::path::PathBuf::from(&output_dir).join(format!("{}.mp4", safe_title)).to_string_lossy().to_string();

    info!(url = %url, output = %output_path, "Downloading with yt-dlp");

    // Write cookies to Netscape format temp file for yt-dlp
    let cookie_file = if !cookies_str.is_empty() {
        let cookie_path = std::path::PathBuf::from(&output_dir).join(".ytdlp_cookies.txt").to_string_lossy().to_string();
        let domain = url.strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .and_then(|s| s.split('/').next())
            .map(|host| {
                let parts: Vec<&str> = host.split('.').collect();
                if parts.len() >= 2 {
                    format!(".{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
                } else {
                    format!(".{}", host)
                }
            })
            .unwrap_or_else(|| ".example.com".to_string());

        let mut netscape = String::from("# Netscape HTTP Cookie File\n");
        for cookie in cookies_str.split(';') {
            let cookie = cookie.trim();
            if let Some((name, value)) = cookie.split_once('=') {
                netscape.push_str(&format!(
                    "{}\tTRUE\t/\tFALSE\t0\t{}\t{}\n",
                    domain,
                    name.trim(),
                    value.trim()
                ));
            }
        }
        let _ = std::fs::write(&cookie_path, &netscape);
        Some(cookie_path)
    } else {
        None
    };

    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.arg("-f").arg(format)
        .arg("--merge-output-format").arg("mp4")
        .arg("--embed-thumbnail")
        .arg("-o").arg(&output_path);

    if let Some(ref cf) = cookie_file {
        cmd.arg("--cookies").arg(cf);
    }

    cmd.arg(&url);

    let status = cmd
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .map_err(|e| CliError::command_execution(format!("Failed to run yt-dlp: {}", e)))?;

    let file_size = std::fs::metadata(&output_path)
        .map(|m| {
            let bytes = m.len();
            if bytes > 1_000_000_000 { format!("{:.1} GB", bytes as f64 / 1e9) }
            else if bytes > 1_000_000 { format!("{:.1} MB", bytes as f64 / 1e6) }
            else { format!("{:.0} KB", bytes as f64 / 1e3) }
        })
        .unwrap_or_else(|_| "-".to_string());

    let result_status = if status.success() { "ok" } else { "failed" };

    // Clean up cookie file
    if let Some(ref cf) = cookie_file {
        let _ = std::fs::remove_file(cf);
    }

    debug!(status = %result_status, size = %file_size, "yt-dlp download complete");

    Ok(serde_json::json!([{
        "title": title,
        "status": result_status,
        "size": file_size,
        "path": output_path,
    }]))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_download_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(DownloadStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_args() -> HashMap<String, Value> {
        HashMap::new()
    }

    #[tokio::test]
    async fn test_download_step_registers() {
        let mut registry = StepRegistry::new();
        register_download_steps(&mut registry);
        assert!(registry.get("download").is_some());
    }

    #[test]
    fn test_download_is_browser_step() {
        assert!(DownloadStep.is_browser_step());
    }

    #[tokio::test]
    async fn test_download_with_url_in_params() {
        let step = DownloadStep;
        let params = json!({"type": "media", "url": "https://example.com/video.mp4"});
        let result = step
            .execute(None, &params, &json!(null), &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_url"], "https://example.com/video.mp4");
        assert_eq!(result["download_path"], "video.mp4");
        assert_eq!(result["download_type"], "media");
        assert_eq!(result["download_status"], "pending");
    }

    #[tokio::test]
    async fn test_download_with_url_in_data() {
        let step = DownloadStep;
        let params = json!({"type": "article"});
        let data = json!({"url": "https://example.com/article.pdf", "title": "Test"});
        let result = step
            .execute(None, &params, &data, &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_url"], "https://example.com/article.pdf");
        assert_eq!(result["download_path"], "article.pdf");
        assert_eq!(result["download_type"], "article");
        assert_eq!(result["title"], "Test");
    }

    #[tokio::test]
    async fn test_download_no_url() {
        let step = DownloadStep;
        let result = step
            .execute(None, &json!(null), &json!(null), &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_status"], "pending");
        assert!(result.get("download_url").is_none());
    }
}
