// Architecture and protocol design derived from OpenCLI
// (https://github.com/jackwener/opencli) by jackwener, Apache-2.0

pub mod types;
pub mod daemon_client;
pub mod page;
pub mod dom_helpers;
pub mod stealth;
pub mod daemon;
pub mod bridge;
pub mod cdp;

pub use bridge::BrowserBridge;
pub use page::DaemonPage;
pub use cdp::CdpPage;
pub use daemon::Daemon;
pub use daemon_client::DaemonClient;
pub use types::{DaemonCommand, DaemonResult, ReadArticle};
