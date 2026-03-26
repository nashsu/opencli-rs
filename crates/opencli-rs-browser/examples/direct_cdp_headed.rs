//! End-to-end test: headed browser with CDP and cookie sync.
//!
//! Run with: cargo run -p opencli-rs-browser --example direct_cdp_headed

use opencli_rs_browser::browser_launcher;
use opencli_rs_browser::CdpPage;
use opencli_rs_core::IPage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Headed CDP Test ===\n");

    println!("[1] Launching headed browser with CDP...");
    let endpoint = browser_launcher::connect_or_launch_with(false).await?;
    println!(
        "    endpoint: {} (launched: {})\n",
        endpoint.ws_url, endpoint.launched
    );

    let page = CdpPage::connect(&endpoint.ws_url).await?;

    let url = "https://x.com/search?q=rust+programming&f=live";
    println!("[2] Navigating to {url}...");
    page.goto(url, None).await?;
    page.wait_for_timeout(5000).await?;

    let title = page.title().await?;
    let current_url = page.url().await?;
    println!("    Title: {title}");
    println!("    URL:   {current_url}\n");

    if current_url.contains("/login") {
        println!("    ⚠ Redirected to login — cookies not available");
    } else {
        println!("    ✓ Authenticated!\n");

        println!("[3] Extracting tweets...\n");
        let js = r#"
            (function() {
                const tweets = [];
                const articles = document.querySelectorAll('article[data-testid="tweet"]');
                articles.forEach((article, i) => {
                    if (i >= 5) return;
                    const userEl = article.querySelector('[data-testid="User-Name"]');
                    const textEl = article.querySelector('[data-testid="tweetText"]');
                    const user = userEl ? userEl.innerText.replace(/\n/g, ' ') : '?';
                    const text = textEl ? textEl.innerText : '';
                    if (text) { tweets.push({ user: user, text: text }); }
                });
                return JSON.stringify(tweets);
            })()
        "#;
        let result = page.evaluate(js).await?;
        let json_str = result.as_str().unwrap_or("[]");
        let tweets: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_default();
        for (i, tweet) in tweets.iter().enumerate() {
            println!(
                "--- Tweet {} ---\n  @{}\n  {}\n",
                i + 1,
                tweet["user"].as_str().unwrap_or("?"),
                tweet["text"].as_str().unwrap_or("")
            );
        }
    }

    println!("=== Done (browser window stays open) ===");
    Ok(())
}
