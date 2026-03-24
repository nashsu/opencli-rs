use crate::yaml_parser::parse_yaml_adapter;
use opencli_rs_core::{CliError, Registry};

include!(concat!(env!("OUT_DIR"), "/builtin_adapters.rs"));

pub fn discover_builtin_adapters(registry: &mut Registry) -> Result<usize, CliError> {
    let mut count = 0;
    for (path, content) in BUILTIN_ADAPTERS {
        match parse_yaml_adapter(content) {
            Ok(cmd) => {
                tracing::debug!(site = %cmd.site, name = %cmd.name, path = %path, "Registered builtin adapter");
                registry.register(cmd);
                count += 1;
            }
            Err(e) => {
                tracing::warn!(path = %path, error = %e, "Failed to parse builtin adapter");
            }
        }
    }
    Ok(count)
}
