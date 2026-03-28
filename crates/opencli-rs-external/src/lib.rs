pub mod executor;
pub mod loader;
pub mod types;

pub use executor::{execute_external_cli, is_binary_installed};
pub use loader::load_external_clis;
pub use types::ExternalCli;
