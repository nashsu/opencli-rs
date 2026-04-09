use autocli_browser::DaemonClient;
use autocli_external::is_binary_installed;
use colored::Colorize;

pub async fn run_doctor() {
    println!("{}", "autocli diagnostics".bold());
    println!();

    // 1. Check supported Chromium-family browser installed
    let chrome = if cfg!(target_os = "macos") {
        is_binary_installed("google-chrome")
            || is_binary_installed("chromium")
            || is_binary_installed("brave")
            || is_binary_installed("microsoft-edge")
            || std::path::Path::new("/Applications/Google Chrome.app").exists()
            || std::path::Path::new("/Applications/Brave Browser.app").exists()
            || std::path::Path::new("/Applications/Microsoft Edge.app").exists()
    } else if cfg!(target_os = "windows") {
        std::path::Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe").exists()
            || std::path::Path::new(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe")
                .exists()
            || std::path::Path::new(
                r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
            )
            .exists()
            || std::path::Path::new(
                r"C:\Program Files (x86)\BraveSoftware\Brave-Browser\Application\brave.exe",
            )
            .exists()
            || std::path::Path::new(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe")
                .exists()
            || std::path::Path::new(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
                .exists()
            || is_binary_installed("chrome")
            || is_binary_installed("brave")
            || is_binary_installed("msedge")
            || is_binary_installed("chromium")
    } else {
        // Linux
        is_binary_installed("google-chrome")
            || is_binary_installed("google-chrome-stable")
            || is_binary_installed("chromium")
            || is_binary_installed("chromium-browser")
            || is_binary_installed("brave")
            || is_binary_installed("brave-browser")
            || is_binary_installed("microsoft-edge")
            || is_binary_installed("microsoft-edge-stable")
    };
    print_check("Chrome/Chromium/Brave/Edge", chrome);

    // 2. Check daemon reachable
    let client = DaemonClient::new(
        std::env::var("AUTOCLI_DAEMON_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(19925),
    );
    let daemon_running = client.is_running().await;
    print_check("Daemon running", daemon_running);

    // 3. Check extension connected
    if daemon_running {
        let ext_connected = client.is_extension_connected().await;
        print_check("Browser extension connected", ext_connected);
    } else {
        print_check("Browser extension connected", false);
    }

    // 4. Check external CLIs
    println!();
    println!("{}", "External CLIs:".bold());
    for name in &["gh", "docker", "kubectl"] {
        let installed = is_binary_installed(name);
        print_check(name, installed);
    }

    // 5. Check CDP endpoint
    let cdp = std::env::var("AUTOCLI_CDP_ENDPOINT").ok();
    if let Some(endpoint) = cdp {
        println!();
        println!("CDP endpoint: {}", endpoint);
    }

    // 6. Print adapter stats
    println!();
    println!("{}", "Adapter stats:".bold());
    // Will be filled in by main.rs passing registry info
}

fn print_check(label: &str, ok: bool) {
    if ok {
        println!("  {} {}", "✓".green(), label);
    } else {
        println!("  {} {}", "✗".red(), label);
    }
}
