// Architecture and protocol design derived from OpenCLI
// (https://github.com/jackwener/opencli) by jackwener, Apache-2.0

pub mod bridge;
pub mod cdp;
pub mod daemon;
pub mod daemon_client;
pub mod dom_helpers;
pub mod page;
pub mod stealth;
pub mod types;

pub use bridge::BrowserBridge;
pub use cdp::CdpPage;
pub use daemon::Daemon;
pub use daemon_client::DaemonClient;
pub use page::DaemonPage;
pub use types::{DaemonCommand, DaemonResult, ReadArticle};
