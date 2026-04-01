use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::output::OutputFormat;
use crate::sql::OperationClass;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_profile: Option<String>,

    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub policies: BTreeMap<String, PolicyConfig>,

    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OutputConfig {
    #[serde(default)]
    pub default_format: Option<OutputFormat>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileConfig {
    pub kind: DatabaseKind,
    pub url: String,

    #[serde(default)]
    pub policy: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub allow: BTreeSet<OperationClass>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseKind {
    Mysql,
    Postgres,
    Sqlite,
}

#[derive(Debug)]
pub struct ResolvedProfile<'a> {
    pub name: &'a str,
    pub config: &'a ProfileConfig,
    pub policy: PermissionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    name: String,
    allowed: BTreeSet<OperationClass>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("unable to read `{}`", path.display()))?;
        let parsed = toml::from_str::<Self>(&raw)
            .with_context(|| format!("invalid TOML in `{}`", path.display()))?;

        if parsed.profiles.is_empty() {
            bail!("config must define at least one profile");
        }

        Ok(parsed)
    }

    pub fn resolve_profile<'a>(
        &'a self,
        requested: Option<&'a str>,
    ) -> Result<ResolvedProfile<'a>> {
        let name = match requested {
            Some(name) => name,
            None => match self.default_profile.as_deref() {
                Some(name) => name,
                None if self.profiles.len() == 1 => self
                    .profiles
                    .keys()
                    .next()
                    .map(String::as_str)
                    .ok_or_else(|| anyhow!("config must define at least one profile"))?,
                None => {
                    bail!(
                        "multiple profiles found; set `default_profile` in the config or pass `--profile`"
                    )
                }
            },
        };

        let config = self
            .profiles
            .get(name)
            .ok_or_else(|| anyhow!("profile `{name}` not found"))?;
        let policy_name = config.policy.as_deref().unwrap_or("all");
        let policy = self.resolve_policy(policy_name)?;

        Ok(ResolvedProfile {
            name,
            config,
            policy,
        })
    }

    fn resolve_policy(&self, name: &str) -> Result<PermissionPolicy> {
        if let Some(policy) = self.policies.get(name) {
            return Ok(PermissionPolicy::new(name, policy.allow.iter().copied()));
        }

        PermissionPolicy::builtin(name).ok_or_else(|| anyhow!("policy `{name}` not found"))
    }
}

pub fn resolve_config_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }

    let candidates = default_config_candidates()?;
    candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "no config file found; looked in {}; pass `--config` explicitly",
                candidates
                    .iter()
                    .map(|path| format!("`{}`", path.display()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn default_config_candidates() -> Result<Vec<PathBuf>> {
    let mut candidates = vec![PathBuf::from("dbx.toml")];
    let home_dir = dirs::home_dir();

    if let Some(home_dir) = home_dir.as_ref() {
        candidates.push(home_dir.join(".dbx").join("config.toml"));
    }

    if let Some(xdg_dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(xdg_dir).join("dbx").join("config.toml"));
    } else if let Some(home_dir) = home_dir {
        candidates.push(home_dir.join(".config").join("dbx").join("config.toml"));
    }

    if candidates.len() == 1 {
        bail!("could not determine a default config path; pass `--config` explicitly");
    }

    Ok(candidates)
}

impl fmt::Display for DatabaseKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Mysql => "mysql",
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        };

        write!(f, "{value}")
    }
}

impl PermissionPolicy {
    pub fn new<I>(name: impl Into<String>, allowed: I) -> Self
    where
        I: IntoIterator<Item = OperationClass>,
    {
        Self {
            name: name.into(),
            allowed: allowed.into_iter().collect(),
        }
    }

    pub fn builtin(name: &str) -> Option<Self> {
        let policy = match name {
            "all" => Self::new(
                name,
                [
                    OperationClass::Read,
                    OperationClass::DmlWrite,
                    OperationClass::SchemaInspect,
                    OperationClass::SchemaChange,
                    OperationClass::Explain,
                ],
            ),
            "readonly" | "prod_safe" => Self::new(
                name,
                [
                    OperationClass::Read,
                    OperationClass::SchemaInspect,
                    OperationClass::Explain,
                ],
            ),
            "migration_only" => Self::new(name, [OperationClass::SchemaChange]),
            _ => return None,
        };

        Some(policy)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn allows(&self, class: OperationClass) -> bool {
        self.allowed.contains(&class)
    }

    pub fn allowed(&self) -> &BTreeSet<OperationClass> {
        &self.allowed
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        path::PathBuf,
        sync::{Mutex, OnceLock},
    };

    use super::{
        Config, DatabaseKind, PermissionPolicy, default_config_candidates, resolve_config_path,
    };
    use crate::sql::OperationClass;
    use tempfile::tempdir;

    fn process_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_config(contents: &str) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), contents).unwrap();
        file
    }

    fn restore_env_var(key: &str, value: Option<OsString>) {
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    #[test]
    fn loads_config_and_resolves_custom_policy() {
        let file = write_config(
            r#"
default_profile = "prod"

[policies.readonly_custom]
allow = ["read", "schema_inspect"]

[profiles.prod]
kind = "postgres"
url = "postgres://localhost/app"
policy = "readonly_custom"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let profile = config.resolve_profile(None).unwrap();

        assert_eq!(profile.name, "prod");
        assert_eq!(profile.config.kind, DatabaseKind::Postgres);
        assert!(profile.policy.allows(OperationClass::Read));
        assert!(profile.policy.allows(OperationClass::SchemaInspect));
        assert!(!profile.policy.allows(OperationClass::Explain));
    }

    #[test]
    fn resolves_single_profile_without_default() {
        let file = write_config(
            r#"
[profiles.local]
kind = "sqlite"
url = "sqlite://dev.db"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let profile = config.resolve_profile(None).unwrap();
        assert_eq!(profile.name, "local");
        assert_eq!(profile.config.kind, DatabaseKind::Sqlite);
        assert_eq!(profile.policy.name(), "all");
    }

    #[test]
    fn errors_when_multiple_profiles_have_no_default() {
        let file = write_config(
            r#"
[profiles.a]
kind = "sqlite"
url = "sqlite://a.db"

[profiles.b]
kind = "mysql"
url = "mysql://localhost/b"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let err = config.resolve_profile(None).unwrap_err();
        assert!(err.to_string().contains(
            "multiple profiles found; set `default_profile` in the config or pass `--profile`"
        ));
    }

    #[test]
    fn errors_on_unknown_profile() {
        let file = write_config(
            r#"
[profiles.local]
kind = "sqlite"
url = "sqlite://dev.db"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let err = config.resolve_profile(Some("missing")).unwrap_err();
        assert!(err.to_string().contains("profile `missing` not found"));
    }

    #[test]
    fn errors_on_unknown_policy() {
        let file = write_config(
            r#"
[profiles.prod]
kind = "postgres"
url = "postgres://localhost/app"
policy = "missing"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let err = config.resolve_profile(None).unwrap_err();
        assert!(err.to_string().contains("policy `missing` not found"));
    }

    #[test]
    fn builtin_policies_have_expected_permissions() {
        let readonly = PermissionPolicy::builtin("readonly").unwrap();
        let migration = PermissionPolicy::builtin("migration_only").unwrap();

        assert!(readonly.allows(OperationClass::Read));
        assert!(readonly.allows(OperationClass::Explain));
        assert!(!readonly.allows(OperationClass::DmlWrite));
        assert!(migration.allows(OperationClass::SchemaChange));
        assert_eq!(migration.allowed().len(), 1);
    }

    #[test]
    fn load_requires_at_least_one_profile() {
        let file = write_config(
            r#"
[output]
default_format = "json"
"#,
        );

        let err = Config::load(file.path()).unwrap_err();
        assert!(
            err.to_string()
                .contains("config must define at least one profile")
        );
    }

    #[test]
    fn resolve_config_path_prefers_explicit_path() {
        let explicit = PathBuf::from("/tmp/dbx-explicit.toml");
        let resolved = resolve_config_path(Some(&explicit)).unwrap();
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_config_path_prefers_local_file() {
        let _guard = process_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        std::fs::write("dbx.toml", "ignored").unwrap();

        let resolved = resolve_config_path(None).unwrap();

        std::env::set_current_dir(original).unwrap();
        assert_eq!(resolved, PathBuf::from("dbx.toml"));
    }

    #[test]
    fn resolve_config_path_prefers_home_dbx_file_over_xdg_config() {
        let _guard = process_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let home_dir = dir.path().join("home");
        let xdg_dir = dir.path().join("xdg");
        let home_config = home_dir.join(".dbx").join("config.toml");
        let xdg_config = xdg_dir.join("dbx").join("config.toml");
        std::fs::create_dir_all(home_config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(xdg_config.parent().unwrap()).unwrap();
        std::fs::write(&home_config, "ignored").unwrap();
        std::fs::write(&xdg_config, "ignored").unwrap();

        let original_home = std::env::var_os("HOME");
        let original_xdg = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::set_var("XDG_CONFIG_HOME", &xdg_dir);
        }

        let resolved = resolve_config_path(None).unwrap();

        restore_env_var("HOME", original_home);
        restore_env_var("XDG_CONFIG_HOME", original_xdg);
        assert_eq!(resolved, home_config);
    }

    #[test]
    fn resolve_config_path_falls_back_to_xdg_when_home_dbx_file_is_missing() {
        let _guard = process_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let home_dir = dir.path().join("home");
        let xdg_dir = dir.path().join("xdg");
        let xdg_config = xdg_dir.join("dbx").join("config.toml");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(xdg_config.parent().unwrap()).unwrap();
        std::fs::write(&xdg_config, "ignored").unwrap();

        let original_home = std::env::var_os("HOME");
        let original_xdg = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::set_var("XDG_CONFIG_HOME", &xdg_dir);
        }

        let resolved = resolve_config_path(None).unwrap();

        restore_env_var("HOME", original_home);
        restore_env_var("XDG_CONFIG_HOME", original_xdg);
        assert_eq!(resolved, xdg_config);
    }

    #[test]
    fn default_config_candidates_use_home_dot_config_when_xdg_env_is_missing() {
        let _guard = process_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let home_dir = dir.path().join("home");
        let expected = home_dir.join(".config").join("dbx").join("config.toml");
        std::fs::create_dir_all(&home_dir).unwrap();

        let original_home = std::env::var_os("HOME");
        let original_xdg = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let candidates = default_config_candidates().unwrap();

        restore_env_var("HOME", original_home);
        restore_env_var("XDG_CONFIG_HOME", original_xdg);
        assert_eq!(candidates[2], expected);
    }

    #[test]
    fn resolve_config_path_errors_when_no_default_config_exists() {
        let _guard = process_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let home_dir = dir.path().join("home");
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        std::fs::create_dir_all(&home_dir).unwrap();

        let original_home = std::env::var_os("HOME");
        let original_xdg = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let err = resolve_config_path(None).unwrap_err();

        std::env::set_current_dir(original).unwrap();
        restore_env_var("HOME", original_home);
        restore_env_var("XDG_CONFIG_HOME", original_xdg);
        assert!(err.to_string().contains("no config file found; looked in"));
        assert!(err.to_string().contains("dbx.toml"));
        assert!(err.to_string().contains(".dbx/config.toml"));
        assert!(err.to_string().contains(".config/dbx/config.toml"));
    }
}
