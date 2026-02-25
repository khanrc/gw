use serde::Deserialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub gc: GcConfig,
    #[serde(default)]
    pub verify: VerifyConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Defaults {
    pub base: Option<String>,
    pub worktrees_dir: Option<String>,
    pub branch_prefix: Option<String>,
    pub subdir: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GcConfig {
    pub stale_days: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VerifyConfig {
    pub rust: Option<String>,
    pub node: Option<String>,
    pub python: Option<String>,
}

impl Config {
    pub fn load(repo_root: &Path) -> anyhow::Result<Self> {
        let mut config = Config::default();

        if let Some(global_path) = global_config_path() {
            if global_path.exists() {
                let data = fs::read_to_string(&global_path)?;
                let parsed: Config = toml::from_str(&data)?;
                config = merge(config, parsed);
            }
        }

        let project_path = repo_root.join(".gw").join("config.toml");
        if project_path.exists() {
            let data = fs::read_to_string(&project_path)?;
            let parsed: Config = toml::from_str(&data)?;
            config = merge(config, parsed);
        }

        Ok(config)
    }

    pub fn worktrees_dir(&self) -> String {
        if let Ok(value) = env::var("GW_WORKTREES_DIR") {
            return value;
        }
        self.defaults
            .worktrees_dir
            .clone()
            .unwrap_or_else(|| ".worktrees".to_string())
    }

    pub fn branch_prefix(&self) -> String {
        self.defaults
            .branch_prefix
            .clone()
            .unwrap_or_else(|| "logan/".to_string())
    }

    pub fn default_base(&self) -> Option<String> {
        if let Ok(value) = env::var("GW_DEFAULT_BASE") {
            return Some(value);
        }
        self.defaults.base.clone()
    }

    pub fn default_subdir(&self) -> Option<String> {
        if let Ok(value) = env::var("GW_SUBDIR") {
            return Some(value);
        }
        self.defaults.subdir.clone()
    }

    pub fn gc_stale_days(&self) -> i64 {
        self.gc.stale_days.unwrap_or(7)
    }

    pub fn verify_rust(&self) -> String {
        self.verify
            .rust
            .clone()
            .unwrap_or_else(|| "cargo test".to_string())
    }

    pub fn verify_node(&self) -> String {
        self.verify
            .node
            .clone()
            .unwrap_or_else(|| "npm test".to_string())
    }

    pub fn verify_python(&self) -> String {
        self.verify
            .python
            .clone()
            .unwrap_or_else(|| "pytest".to_string())
    }

    pub fn validate(repo_root: &Path) -> Vec<String> {
        let mut warnings = Vec::new();

        let known_sections: HashSet<&str> =
            ["defaults", "gc", "verify"].iter().copied().collect();
        let known_keys: HashSet<&str> = [
            "defaults.base",
            "defaults.worktrees_dir",
            "defaults.branch_prefix",
            "defaults.subdir",
            "gc.stale_days",
            "verify.rust",
            "verify.node",
            "verify.python",
        ]
        .iter()
        .copied()
        .collect();

        let all_key_names: Vec<&str> = known_keys
            .iter()
            .filter_map(|k| k.split('.').nth(1))
            .collect();

        let project_path = repo_root.join(".gw").join("config.toml");
        if !project_path.exists() {
            return warnings;
        }
        let data = match fs::read_to_string(&project_path) {
            Ok(d) => d,
            Err(_) => return warnings,
        };
        let value: toml::Value = match data.parse() {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!(".gw/config.toml: parse error: {}", e));
                return warnings;
            }
        };

        if let Some(table) = value.as_table() {
            for (section, val) in table {
                if !known_sections.contains(section.as_str()) {
                    warnings.push(format!(
                        ".gw/config.toml: unknown section '{}'",
                        section
                    ));
                    continue;
                }
                if let Some(inner) = val.as_table() {
                    for key in inner.keys() {
                        let full = format!("{}.{}", section, key);
                        if !known_keys.contains(full.as_str()) {
                            let suggestion = suggest_key(key, &all_key_names);
                            if let Some(s) = suggestion {
                                warnings.push(format!(
                                    ".gw/config.toml: unknown key '{}' (did you mean '{}'?)",
                                    full, s
                                ));
                            } else {
                                warnings.push(format!(
                                    ".gw/config.toml: unknown key '{}'",
                                    full
                                ));
                            }
                        }
                    }
                }
            }

            // Value validation
            if let Some(gc) = table.get("gc").and_then(|v| v.as_table()) {
                if let Some(days) = gc.get("stale_days").and_then(|v| v.as_integer()) {
                    if days <= 0 {
                        warnings.push(
                            ".gw/config.toml: 'gc.stale_days' should be positive".to_string(),
                        );
                    }
                }
            }
            if let Some(defaults) = table.get("defaults").and_then(|v| v.as_table()) {
                if let Some(subdir) = defaults.get("subdir").and_then(|v| v.as_str()) {
                    if subdir.starts_with('/') {
                        warnings.push(
                            ".gw/config.toml: 'defaults.subdir' should not start with '/'".to_string(),
                        );
                    }
                }
            }
        }

        warnings
    }
}

fn merge(base: Config, override_cfg: Config) -> Config {
    Config {
        defaults: Defaults {
            base: override_cfg.defaults.base.or(base.defaults.base),
            worktrees_dir: override_cfg
                .defaults
                .worktrees_dir
                .or(base.defaults.worktrees_dir),
            branch_prefix: override_cfg
                .defaults
                .branch_prefix
                .or(base.defaults.branch_prefix),
            subdir: override_cfg.defaults.subdir.or(base.defaults.subdir),
        },
        gc: GcConfig {
            stale_days: override_cfg.gc.stale_days.or(base.gc.stale_days),
        },
        verify: VerifyConfig {
            rust: override_cfg.verify.rust.or(base.verify.rust),
            node: override_cfg.verify.node.or(base.verify.node),
            python: override_cfg.verify.python.or(base.verify.python),
        },
    }
}

fn suggest_key(input: &str, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .map(|c| (c, strsim::levenshtein(input, c)))
        .filter(|(_, d)| *d <= 2)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c.to_string())
}

pub fn gw_home() -> Option<PathBuf> {
    if let Ok(home) = env::var("GW_HOME") {
        return Some(PathBuf::from(home));
    }
    let home = home_dir()?;
    Some(home.join(".gw"))
}

fn global_config_path() -> Option<PathBuf> {
    let home = gw_home()?;
    Some(home.join("config.toml"))
}

fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = env::var("HOME") {
        return Some(PathBuf::from(home));
    }
    if let Ok(profile) = env::var("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn validate_empty_config_no_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let gw_dir = dir.path().join(".gw");
        fs::create_dir_all(&gw_dir).unwrap();
        fs::write(gw_dir.join("config.toml"), "").unwrap();
        let warnings = Config::validate(dir.path());
        assert!(warnings.is_empty());
    }

    #[test]
    fn validate_valid_config_no_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let gw_dir = dir.path().join(".gw");
        fs::create_dir_all(&gw_dir).unwrap();
        fs::write(
            gw_dir.join("config.toml"),
            r#"
[defaults]
subdir = "services/app"
worktrees_dir = ".wt"

[gc]
stale_days = 14

[verify]
rust = "cargo check"
"#,
        )
        .unwrap();
        let warnings = Config::validate(dir.path());
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
    }

    #[test]
    fn validate_unknown_key_detected() {
        let dir = tempfile::tempdir().unwrap();
        let gw_dir = dir.path().join(".gw");
        fs::create_dir_all(&gw_dir).unwrap();
        fs::write(
            gw_dir.join("config.toml"),
            r#"
[defaults]
subddir = "services/app"
"#,
        )
        .unwrap();
        let warnings = Config::validate(dir.path());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown key"));
        assert!(warnings[0].contains("did you mean 'subdir'"));
    }

    #[test]
    fn validate_unknown_section_detected() {
        let dir = tempfile::tempdir().unwrap();
        let gw_dir = dir.path().join(".gw");
        fs::create_dir_all(&gw_dir).unwrap();
        fs::write(
            gw_dir.join("config.toml"),
            r#"
[unknown_section]
key = "value"
"#,
        )
        .unwrap();
        let warnings = Config::validate(dir.path());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown section"));
    }

    #[test]
    fn validate_stale_days_positive() {
        let dir = tempfile::tempdir().unwrap();
        let gw_dir = dir.path().join(".gw");
        fs::create_dir_all(&gw_dir).unwrap();
        fs::write(
            gw_dir.join("config.toml"),
            r#"
[gc]
stale_days = -1
"#,
        )
        .unwrap();
        let warnings = Config::validate(dir.path());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("should be positive"));
    }

    #[test]
    fn validate_subdir_no_leading_slash() {
        let dir = tempfile::tempdir().unwrap();
        let gw_dir = dir.path().join(".gw");
        fs::create_dir_all(&gw_dir).unwrap();
        fs::write(
            gw_dir.join("config.toml"),
            r#"
[defaults]
subdir = "/services/app"
"#,
        )
        .unwrap();
        let warnings = Config::validate(dir.path());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("should not start with '/'"));
    }

    #[test]
    fn validate_no_config_file_no_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let warnings = Config::validate(dir.path());
        assert!(warnings.is_empty());
    }

    #[test]
    fn merge_subdir_override() {
        let base = Config {
            defaults: Defaults {
                subdir: Some("services/a".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let over = Config {
            defaults: Defaults {
                subdir: Some("services/b".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = merge(base, over);
        assert_eq!(result.defaults.subdir.unwrap(), "services/b");
    }

    #[test]
    fn merge_subdir_fallback() {
        let base = Config {
            defaults: Defaults {
                subdir: Some("services/a".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let over = Config::default();
        let result = merge(base, over);
        assert_eq!(result.defaults.subdir.unwrap(), "services/a");
    }
}
