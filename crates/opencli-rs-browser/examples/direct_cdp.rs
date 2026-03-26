//! End-to-end test: connect via direct CDP and interact with the browser.
//!
//! Run with: cargo run -p opencli-rs-browser --example direct_cdp
//!
//! This example:
//! 1. Discovers an existing CDP endpoint or launches a browser
//! 2. Connects via WebSocket (direct CDP, no extension needed)
//! 3. Navigates to a URL
//! 4. Reads the page title and URL
//! 5. Takes a DOM snapshot
//! 6. Lists open tabs

use opencli_rs_browser::browser_launcher;
use opencli_rs_browser::CdpPage;
use opencli_rs_core::IPage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Direct CDP Connection Test ===\n");

    // Step 1: Get CDP endpoint
    println!("[1] Discovering or launching browser with CDP...");
    let endpoint = browser_launcher::connect_or_launch().await?;
    println!(
        "    CDP endpoint: {} (launched: {})\n",
        endpoint.ws_url, endpoint.launched
    );

    // Step 2: Connect via WebSocket
    println!("[2] Connecting to CDP WebSocket...");
    let page = CdpPage::connect(&endpoint.ws_url).await?;
    println!("    Connected!\n");

    // Step 3: Navigate
    let url = "https://x.com/search?q=rust+programming&f=live";
    println!("[3] Navigating to {url}...");
    page.goto(url, None).await?;
    // Wait for page to load
    page.wait_for_timeout(3000).await?;
    println!("    Done.\n");

    // Step 4: Read title and URL
    let title = page.title().await?;
    let current_url = page.url().await?;
    println!("[4] Page info:");
    println!("    Title: {title}");
    println!("    URL:   {current_url}\n");

    // Step 5: DOM snapshot
    println!("[5] Taking DOM snapshot...");
    let snapshot = page.snapshot(None).await?;
    let snapshot_str = serde_json::to_string_pretty(&snapshot)?;
    let preview: String = snapshot_str.chars().take(500).collect();
    println!("    Snapshot preview (first 500 chars):");
    println!("    {preview}...\n");

    // Step 6: List tabs
    println!("[6] Listing open tabs...");
    let tabs = page.tabs().await?;
    for (i, tab) in tabs.iter().enumerate() {
        println!(
            "    Tab {}: {} — {}",
            i,
            tab.title.as_deref().unwrap_or("(untitled)"),
            tab.url
        );
    }

    println!("\n=== Test Complete ===");
    Ok(())
}
