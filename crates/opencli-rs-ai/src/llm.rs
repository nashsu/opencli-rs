//! LLM client for AI-powered adapter generation.
//! Routes all requests through the AutoCLI server API.

use opencli_rs_core::CliError;
use serde_json::{json, Value};
use tracing::{debug, info};

use crate::config::api_base;

/// The system prompt for adapter generation, embedded at compile time.
const SYSTEM_PROMPT: &str = include_str!("../prompts/generate-adapter.md");

/// Send captured page data to LLM via server API and get back a YAML adapter.
pub async fn generate_with_llm(
    token: &str,
    captured_data: &Value,
    goal: &str,
    site: &str,
) -> Result<String, CliError> {
    let endpoint = format!("{}/api/ai/v1/chat/completions", api_base());

    let user_message = format!(
        "Generate an opencli-rs YAML adapter for site \"{}\" with goal \"{}\".\n\n\
        CRITICAL RULES:\n\
        1. The `name` field MUST be exactly \"{}\".\n\
        2. You MUST include a `tags` field with at least 3 English classification tags for the website (e.g. tags: [technology, programming, blog]).\n\
        3. Choose the best extraction approach: DOM scraping OR API calls. \
        If the HTML has structured data, use DOM scraping. \
        If you use API calls, you MUST strictly replicate the original page's request — same HTTP method, same headers, same body, same URL (use Performance API to find it).\n\
        4. Only add required args when the goal genuinely needs user input.\n\n\
        Here is the captured data from the web page:\n\n```json\n{}\n```\n\n\
        Return ONLY the YAML content, no explanation, no markdown fencing. Just the raw YAML.",
        site, goal, goal,
        serde_json::to_string_pretty(captured_data)
            .unwrap_or_else(|_| captured_data.to_string())
    );

    info!(endpoint = %endpoint, "Calling LLM for adapter generation");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| CliError::Http { message: format!("Failed to create HTTP client: {}", e), suggestions: vec![], source: None })?;

    let request_body = json!({
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user_message }
        ],
        "stream": false
    });

    debug!(body_size = request_body.to_string().len(), "Sending LLM request");

    let resp = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| CliError::Http { message: format!("LLM request failed: {}", e), suggestions: vec![], source: None })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CliError::Http { message: format!("LLM API error {}: {}", status, body.chars().take(500).collect::<String>()), suggestions: vec![], source: None });
    }

    let resp_json: Value = resp.json().await
        .map_err(|e| CliError::Http { message: format!("Failed to parse LLM response: {}", e), suggestions: vec![], source: None })?;

    // Extract content from OpenAI-compatible response format
    let content = if let Some(choices) = resp_json.get("choices") {
        choices.get(0)
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        return Err(CliError::Http { message: "Unexpected LLM response format".into(), suggestions: vec![], source: None });
    };

    // Clean up: remove thinking tags and markdown fencing
    let mut cleaned = content.clone();
    while let Some(start) = cleaned.find("<think>") {
        if let Some(end) = cleaned.find("</think>") {
            cleaned = format!("{}{}", &cleaned[..start], &cleaned[end + 8..]);
        } else {
            cleaned = cleaned[..start].to_string();
            break;
        }
    }
    while let Some(start) = cleaned.find("<thinking>") {
        if let Some(end) = cleaned.find("</thinking>") {
            cleaned = format!("{}{}", &cleaned[..start], &cleaned[end + 11..]);
        } else {
            cleaned = cleaned[..start].to_string();
            break;
        }
    }
    let yaml = cleaned
        .trim()
        .strip_prefix("```yaml").or_else(|| cleaned.trim().strip_prefix("```"))
        .unwrap_or(cleaned.trim())
        .strip_suffix("```")
        .unwrap_or(cleaned.trim())
        .trim()
        .to_string();

    if yaml.is_empty() {
        return Err(CliError::Http { message: "LLM returned empty content".into(), suggestions: vec![], source: None });
    }

    info!(yaml_len = yaml.len(), "LLM generated adapter YAML");
    Ok(yaml)
}
