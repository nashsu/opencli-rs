use crate::CliCommand;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Registry {
    commands: HashMap<String, HashMap<String, CliCommand>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, cmd: CliCommand) {
        self.commands
            .entry(cmd.site.clone())
            .or_default()
            .insert(cmd.name.clone(), cmd);
    }

    pub fn get(&self, site: &str, name: &str) -> Option<&CliCommand> {
        self.commands.get(site)?.get(name)
    }

    pub fn list_sites(&self) -> Vec<&str> {
        let mut sites: Vec<&str> = self.commands.keys().map(|s| s.as_str()).collect();
        sites.sort();
        sites
    }

    pub fn list_commands(&self, site: &str) -> Vec<&CliCommand> {
        self.commands
            .get(site)
            .map(|cmds| {
                let mut v: Vec<&CliCommand> = cmds.values().collect();
                v.sort_by(|a, b| a.name.cmp(&b.name));
                v
            })
            .unwrap_or_default()
    }

    pub fn all_commands(&self) -> Vec<&CliCommand> {
        let mut cmds: Vec<&CliCommand> = self.commands.values().flat_map(|s| s.values()).collect();
        cmds.sort_by(|a, b| (&a.site, &a.name).cmp(&(&b.site, &b.name)));
        cmds
    }

    pub fn site_count(&self) -> usize {
        self.commands.len()
    }

    pub fn command_count(&self) -> usize {
        self.commands.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NavigateBefore, Strategy};

    fn test_cmd(site: &str, name: &str) -> CliCommand {
        CliCommand {
            site: site.into(),
            name: name.into(),
            description: format!("{} {}", site, name),
            domain: None,
            strategy: Strategy::Public,
            browser: false,
            args: vec![],
            columns: vec![],
            pipeline: None,
            func: None,
            timeout_seconds: None,
            navigate_before: NavigateBefore::default(),
        }
    }

    #[test]
    fn test_register_and_get() {
        let mut reg = Registry::new();
        reg.register(test_cmd("hackernews", "top"));
        assert!(reg.get("hackernews", "top").is_some());
        assert!(reg.get("hackernews", "missing").is_none());
    }

    #[test]
    fn test_list_sites() {
        let mut reg = Registry::new();
        reg.register(test_cmd("bilibili", "hot"));
        reg.register(test_cmd("hackernews", "top"));
        assert_eq!(reg.list_sites(), vec!["bilibili", "hackernews"]);
    }

    #[test]
    fn test_command_count() {
        let mut reg = Registry::new();
        reg.register(test_cmd("hn", "top"));
        reg.register(test_cmd("hn", "best"));
        reg.register(test_cmd("reddit", "hot"));
        assert_eq!(reg.site_count(), 2);
        assert_eq!(reg.command_count(), 3);
    }
}
