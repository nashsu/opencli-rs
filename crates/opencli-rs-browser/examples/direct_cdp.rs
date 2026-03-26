//! End-to-end test: connect via direct CDP and interact with the browser.
//!
//! Run with: cargo run -p opencli-rs-browser --example direct_cdp

use opencli_rs_browser::browser_launcher;
use opencli_rs_browser::CdpPage;
use opencli_rs_core::IPage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Direct CDP Connection Test ===\n");

    // Step 1: Get CDP endpoint (reuse existing or launch headless)
    println!("[1] Connecting to CDP...");
    let endpoint = browser_launcher::connect_or_launch().await?;
    println!(
        "    endpoint: {} (launched: {})\n",
        endpoint.ws_url, endpoint.launched
    );

    let page = CdpPage::connect(&endpoint.ws_url).await?;

    // Step 2: Navigate to X search
    let url = "https://x.com/search?q=rust+programming&f=live";
    println!("[2] Navigating to {url}...");
    page.goto(url, None).await?;
    page.wait_for_timeout(5000).await?;

    let title = page.title().await?;
    let current_url = page.url().await?;
    println!("    Title: {title}");
    println!("    URL:   {current_url}\n");

    // Step 3: Extract tweet text from the page
    println!("[3] Extracting tweets...\n");
    let js = r#"
        (function() {
            const tweets = [];
            const articles = document.querySelectorAll('article[data-testid="tweet"]');
            articles.forEach((article, i) => {
                if (i >= 10) return;
                const userEl = article.querySelector('[data-testid="User-Name"]');
                const textEl = article.querySelector('[data-testid="tweetText"]');
                const user = userEl ? userEl.innerText.replace(/\n/g, ' ') : '?';
                const text = textEl ? textEl.innerText : '';
                if (text) {
                    tweets.push({ user: user, text: text });
                }
            });
            return JSON.stringify(tweets);
        })()
    "#;

    let result = page.evaluate(js).await?;
    let json_str = result.as_str().unwrap_or("[]");
    let tweets: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_default();

    if tweets.is_empty() {
        println!("    No tweets found (page may still be loading)");
    } else {
        for (i, tweet) in tweets.iter().enumerate() {
            let user = tweet["user"].as_str().unwrap_or("?");
            let text = tweet["text"].as_str().unwrap_or("");
            println!("--- Tweet {} ---", i + 1);
            println!("  @{}", user);
            println!("  {}\n", text);
        }
    }

    println!("=== Done ===");
    Ok(())
}
